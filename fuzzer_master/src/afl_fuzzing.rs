use crate::database::Database;
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::net::{net_execute_sample, net_fuzzing_pretext, ExecuteSampleResult};
use crate::{guarantee_initial_state, power_on, CommandExitResult};
use fuzzer_data::{Code, ExecutionResult, ReportExecutionProblem};
use hypervisor::state::VmExitReason;
use libafl::corpus::{InMemoryCorpus, OnDiskCorpus};
use libafl::events::{SendExiting, SimpleEventManager};
use libafl::executors::{Executor, ExitKind, HasObservers};
use libafl::feedbacks::{CrashFeedback, MaxMapFeedback};
use libafl::generators::RandPrintablesGenerator;
use libafl::inputs::HasTargetBytes;
use libafl::monitors::SimpleMonitor;
use libafl::mutators::{havoc_mutations, HavocScheduledMutator};
use libafl::observers::{MapObserver, Observer, ObserversTuple};
use libafl::schedulers::QueueScheduler;
use libafl::stages::StdMutationalStage;
use libafl::state::{HasExecutions, StdState};
use libafl::{Fuzzer, StdFuzzer};
use libafl_bolts::rands::StdRand;
use libafl_bolts::tuples::{tuple_list, HasConstLen, RefIndexable};
use libafl_bolts::{nonzero, ErrorBacktrace, HasLen, Named};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::mem::transmute;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

static LAST_EXECUTION_RESULT: Mutex<Option<ExecutionResult>> = Mutex::new(None);
static LAST_EXECUTION_PROBLEMS: Mutex<Vec<ReportExecutionProblem>> = Mutex::new(Vec::new());

const HANDLE_UCOVERAGE: Cow<'static, str> = Cow::Borrowed("uCoverage");
const HANDLE_STATE_DIFFERENCE: Cow<'static, str> = Cow::Borrowed("stateDifference");

pub struct AFLExecutor<'a, OT, I, S> {
    udp: &'a mut DeviceConnection,
    interface: &'a Arc<FuzzerNodeInterface>,
    database: &'a mut Database,
    observers: OT,
    phantom: PhantomData<(I, S)>,

    last_code_executed: Option<Code>,
    last_reported_exclusion: Option<(Option<u16>, u16)>, // address, times
}

impl<'a, OT, I, S> AFLExecutor<'a, OT, I, S>
where
    OT: ObserversTuple<I, S>,
{
    fn new(
        udp: &'a mut DeviceConnection,
        interface: &'a Arc<FuzzerNodeInterface>,
        database: &'a mut Database,
        observers: OT,
    ) -> Self {
        AFLExecutor {
            udp,
            interface,
            observers,
            phantom: PhantomData,
            database,
            last_code_executed: None,
            last_reported_exclusion: None,
        }
    }
}

struct ExecuteContext {
    input: Vec<u8>,

    udp: &'static mut DeviceConnection,
    interface: Arc<FuzzerNodeInterface>,
    database: &'static mut Database,
    last_code_executed: &'static mut Option<Code>,
}

impl<'a, EM, I, S, Z, OT> Executor<EM, I, S, Z> for AFLExecutor<'a, OT, I, S>
where
    S: HasExecutions,
    OT: ObserversTuple<I, S>,
    EM: SendExiting,
    I: HasTargetBytes,
{
    fn run_target(
        &mut self,
        _fuzzer: &mut Z,
        state: &mut S,
        _mgr: &mut EM,
        input: &I,
    ) -> Result<ExitKind, libafl::Error> {
        *state.executions_mut() += 1;

        let handle = Handle::current();
        let _guard = handle.enter();

        let mut thread_context = unsafe {
            ExecuteContext {
                input: input.target_bytes().to_vec(),
                udp: transmute(&mut self.udp),
                interface: Arc::clone(&self.interface),
                database: transmute(&mut self.database),
                last_code_executed: transmute(&mut self.last_code_executed),
            }
        };

        let task: JoinHandle<
            Result<(ExecutionResult, Vec<ReportExecutionProblem>), libafl::Error>,
        > = tokio::spawn(async move {
            async fn force_reconnect(
                thread_context: &mut ExecuteContext,
                result: CommandExitResult,
                continue_count: &mut u64,
            ) -> Result<bool, libafl::Error> {
                let connection_lost = match result {
                    CommandExitResult::ForceReconnect => {
                        *continue_count = 0;
                        true
                    }
                    CommandExitResult::RetryOrReconnect => {
                        *continue_count += 1;
                        *continue_count > 3
                    }
                    CommandExitResult::Operational => {
                        *continue_count = 0;
                        false
                    }
                    CommandExitResult::ExitProgram => {
                        info!("Exiting");
                        return Err(libafl::Error::ShuttingDown);
                    }
                };

                if connection_lost {
                    info!("Connection lost");

                    let _ = thread_context.interface.power_button_long().await;

                    // wait for pi reboot

                    let _ = power_on(&thread_context.interface).await;

                    tokio::time::sleep(Duration::from_secs(5)).await;

                    guarantee_initial_state(&thread_context.interface, thread_context.udp).await;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }

            let mut retry_counter = 0u64;
            loop {
                let result = net_fuzzing_pretext(
                    thread_context.udp,
                    thread_context.database,
                    &mut thread_context.last_code_executed,
                    &mut None,
                )
                .await;

                if force_reconnect(&mut thread_context, result, &mut retry_counter).await? {
                    continue;
                }

                *thread_context.last_code_executed = Some(thread_context.input.clone());

                match net_execute_sample(
                    thread_context.udp,
                    &thread_context.interface,
                    thread_context.database,
                    thread_context.input.as_ref(),
                )
                .await
                {
                    ExecuteSampleResult::Timeout => {
                        force_reconnect(
                            &mut thread_context,
                            CommandExitResult::ForceReconnect,
                            &mut retry_counter,
                        )
                        .await?;
                        continue;
                    }
                    ExecuteSampleResult::Rerun => {
                        force_reconnect(
                            &mut thread_context,
                            CommandExitResult::RetryOrReconnect,
                            &mut retry_counter,
                        )
                        .await?;
                        continue;
                    }
                    ExecuteSampleResult::Success((exit, problems)) => {
                        let _ = force_reconnect(
                            &mut thread_context,
                            CommandExitResult::Operational,
                            &mut retry_counter,
                        )
                        .await?;
                        return Ok((exit, problems));
                    }
                };
            }
        });
        let result = futures::executor::block_on(task); // block, here, afterwards unsafe ref not used

        match result {
            Result::Err(e) => {
                error!("Error executing target: {:?}", e);
                return Err(libafl::Error::Runtime(
                    "Error in tokio asyn task".into(),
                    ErrorBacktrace::new(),
                ));
            }
            Result::Ok(result) => {
                let (exit, problems) = result?;

                let mut afl_exit = match exit.exit {
                    VmExitReason::Exception(_)
                    | VmExitReason::Cpuid
                    | VmExitReason::Hlt
                    | VmExitReason::Io
                    | VmExitReason::Rdmsr
                    | VmExitReason::Wrmsr
                    | VmExitReason::Rdrand
                    | VmExitReason::Rdseed
                    | VmExitReason::Rdtsc
                    | VmExitReason::Rdpmc
                    | VmExitReason::Cr8Write
                    | VmExitReason::IoWrite
                    | VmExitReason::MsrUse
                    | VmExitReason::Shutdown(_)
                    | VmExitReason::MonitorTrap
                    | VmExitReason::EPTPageFault(_) => ExitKind::Ok,
                    VmExitReason::VMEntryFailure(_, _)
                    | VmExitReason::Unexpected(_)
                    | VmExitReason::ExternalInterrupt => ExitKind::Crash,
                    VmExitReason::TimerExpiration => ExitKind::Timeout,
                };
                if afl_exit != ExitKind::Timeout {
                    if problems.iter().any(|x| {
                        matches!(
                            x,
                            ReportExecutionProblem::SerializedMismatch { .. }
                                | ReportExecutionProblem::VeryLikelyBug
                        )
                    }) {
                        afl_exit = ExitKind::Crash;
                    }
                }

                if let Ok(mut data) = LAST_EXECUTION_RESULT.lock() {
                    *data = Some(exit);
                }
                if let Ok(mut data) = LAST_EXECUTION_PROBLEMS.lock() {
                    data.clear();
                    data.extend(problems);
                }

                Ok(afl_exit)
            }
        }
    }
}

impl<'a, OT, I, S> HasObservers for AFLExecutor<'a, OT, I, S>
where
    OT: ObserversTuple<I, S>,
{
    type Observers = OT;

    #[inline]
    fn observers(&self) -> RefIndexable<&Self::Observers, Self::Observers> {
        RefIndexable::from(&self.observers)
    }

    #[inline]
    fn observers_mut(&mut self) -> RefIndexable<&mut Self::Observers, Self::Observers> {
        RefIndexable::from(&mut self.observers)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CoverageObserver {
    coverage: BTreeMap<u16, u16>,
    #[serde(skip, default = "default_array")]
    slice: [u8; 0x7c00],
}

fn default_array() -> [u8; 0x7c00] {
    [0; 0x7c00]
}

impl Default for CoverageObserver {
    fn default() -> Self {
        CoverageObserver {
            coverage: BTreeMap::new(),
            slice: [0; 0x7c00],
        }
    }
}

impl<I, S> Observer<I, S> for CoverageObserver {
    fn pre_exec(&mut self, _state: &mut S, _input: &I) -> Result<(), libafl::Error> {
        if let Ok(mut data) = LAST_EXECUTION_RESULT.lock() {
            if data.is_some() {
                *data = None;
            }
        }
        self.reset_map()?;
        Ok(())
    }
    fn post_exec(
        &mut self,
        _state: &mut S,
        _input: &I,
        _exit_kind: &ExitKind,
    ) -> Result<(), libafl::Error> {
        if let Ok(data) = LAST_EXECUTION_RESULT.lock() {
            if let Some(execution_result) = data.as_ref() {
                self.coverage.clone_from(&execution_result.coverage);

                for (address, count) in self.coverage.iter() {
                    if (*address as usize) < self.slice.len() {
                        self.slice[*address as usize] = (*count).max(u8::MAX as u16) as u8;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Named for CoverageObserver {
    fn name(&self) -> &Cow<'static, str> {
        &HANDLE_UCOVERAGE
    }
}

impl HasConstLen for CoverageObserver {
    const LEN: usize = 0x7c00;
}

impl HasLen for CoverageObserver {
    fn len(&self) -> usize {
        Self::LEN
    }
}

impl Hash for CoverageObserver {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.coverage.hash(state)
    }
}

impl MapObserver for CoverageObserver {
    type Entry = u8;

    fn count_bytes(&self) -> u64 {
        self.coverage.values().filter(|&&count| count > 0).count() as u64
    }

    fn get(&self, idx: usize) -> Self::Entry {
        self.coverage
            .get(&(idx as u16))
            .unwrap_or(&0)
            .clone()
            .max(u8::MAX as u16) as u8
    }

    fn usable_count(&self) -> usize {
        0x7c00 / 4 * 3
    }

    fn how_many_set(&self, indexes: &[usize]) -> usize {
        self.coverage
            .iter()
            .filter(|(address, count)| indexes.contains(&(**address as usize)) && **count > 0)
            .count()
    }

    fn initial(&self) -> Self::Entry {
        0
    }

    fn reset_map(&mut self) -> Result<(), libafl::Error> {
        self.coverage.clear();
        self.slice.fill(0);
        Ok(())
    }

    fn set(&mut self, idx: usize, val: Self::Entry) {
        self.coverage.insert(idx as u16, val as u16);
    }

    fn to_vec(&self) -> Vec<Self::Entry> {
        self.slice.to_vec()
    }
}

impl AsRef<Self> for CoverageObserver {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl AsMut<Self> for CoverageObserver {
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

impl Deref for CoverageObserver {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.slice
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DifferenceObserver {
    problems: Vec<ReportExecutionProblem>,
}

impl<I, S> Observer<I, S> for DifferenceObserver {
    fn pre_exec(&mut self, _state: &mut S, _input: &I) -> Result<(), libafl::Error> {
        if let Ok(mut data) = LAST_EXECUTION_PROBLEMS.lock() {
            data.clear();
        }
        Ok(())
    }

    fn post_exec(
        &mut self,
        _state: &mut S,
        _input: &I,
        _exit_kind: &ExitKind,
    ) -> Result<(), libafl::Error> {
        if let Ok(data) = LAST_EXECUTION_PROBLEMS.lock() {
            self.problems.clone_from(&data);
        }
        Ok(())
    }
}

impl Named for DifferenceObserver {
    fn name(&self) -> &Cow<'static, str> {
        &HANDLE_STATE_DIFFERENCE
    }
}

impl Hash for DifferenceObserver {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.problems.hash(state);
    }
}

impl AsRef<Self> for DifferenceObserver {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl AsMut<Self> for DifferenceObserver {
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

pub fn afl_main(
    udp: &mut DeviceConnection,
    interface: &Arc<FuzzerNodeInterface>,
    db: &mut Database,
    _corpus: Option<PathBuf>,
    solution: Option<PathBuf>,
    _timeout_hours: Option<u32>,
    _disable_feedback: bool,
) {
    // The closure that we want to fuzz
    let coverage_observer = CoverageObserver::default();

    // Feedback to rate the interestingness of an input
    let mut feedback = MaxMapFeedback::new(&coverage_observer);

    // A feedback to choose if an input is a solution or not
    let mut objective = CrashFeedback::new();

    // create a State from scratch
    let mut state = StdState::new(
        // RNG
        StdRand::new(),
        // Corpus that will be evolved, we keep it in memory for performance
        InMemoryCorpus::new(),
        // Corpus in which we store solutions (crashes in this example),
        // on disk so the user can get them after stopping the fuzzer
        OnDiskCorpus::new(solution.unwrap_or(PathBuf::from("./findings")))
            .expect("Corpus initialization failed"),
        &mut feedback,
        &mut objective,
    )
    .unwrap();

    // The Monitor trait defines how the fuzzer stats are displayed to the user
    let mon = SimpleMonitor::new(|s| println!("{s}"));

    // The event manager handles the various events generated during the fuzzing loop
    // such as the notification of the addition of a new item to the corpus
    let mut mgr = SimpleEventManager::new(mon);

    // A queue policy to get testcasess from the corpus
    let scheduler = QueueScheduler::new();

    // A fuzzer with feedbacks and a corpus scheduler
    let mut fuzzer = StdFuzzer::new(scheduler, feedback, objective);

    // Create the executor for an in-process function with just one observer
    let mut executor = AFLExecutor::new(udp, interface, db, tuple_list!(coverage_observer));

    // Generator of printable bytearrays of max size 32
    let mut generator = RandPrintablesGenerator::new(nonzero!(32));

    // Generate 8 initial inputs
    state
        .generate_initial_inputs(&mut fuzzer, &mut executor, &mut generator, &mut mgr, 8)
        .expect("Failed to generate the initial corpus");

    // Setup a mutational stage with a basic bytes mutator
    let mutator = HavocScheduledMutator::new(havoc_mutations());
    let mut stages = tuple_list!(StdMutationalStage::new(mutator));

    fuzzer
        .fuzz_loop(&mut stages, &mut executor, &mut state, &mut mgr)
        .expect("Error in the fuzzing loop");
}
