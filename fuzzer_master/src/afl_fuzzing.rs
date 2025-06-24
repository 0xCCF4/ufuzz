//! AFL-style Fuzzing Module
//!
//! This module implements integration with the libafl framework.

use crate::database::Database;
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use crate::net::{net_execute_sample, net_fuzzing_pretext, ExecuteSampleResult};
use crate::{guarantee_initial_state, power_on, CommandExitResult};
use fuzzer_data::instruction_corpus::CorpusInstruction;
use fuzzer_data::{Code, ExecutionResult, OtaC2DTransport, ReportExecutionProblem};
use hypervisor::state::VmExitReason;
use libafl::corpus::{CachedOnDiskCorpus, OnDiskCorpus};
use libafl::events::{SendExiting, SimpleEventManager};
use libafl::executors::{Executor, ExitKind, HasObservers};
use libafl::feedbacks::{CrashFeedback, MaxMapFeedback};
use libafl::generators::{Generator, RandBytesGenerator, RandPrintablesGenerator};
use libafl::inputs::{BytesInput, HasTargetBytes};
use libafl::monitors::SimpleMonitor;
use libafl::mutators::{havoc_mutations, HavocScheduledMutator};
use libafl::observers::{MapObserver, Observer, ObserversTuple};
use libafl::schedulers::QueueScheduler;
use libafl::stages::StdMutationalStage;
use libafl::state::{HasExecutions, HasRand, StdState};
use libafl::{Fuzzer, StdFuzzer};
use libafl_bolts::rands::{Rand, StdRand};
use libafl_bolts::tuples::{tuple_list, HasConstLen, RefIndexable};
use libafl_bolts::{nonzero, ErrorBacktrace, HasLen, Named};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::mem::transmute;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

/// Global state for tracking execution results
static LAST_EXECUTION_RESULT: Mutex<Option<ExecutionResult>> = Mutex::new(None);
static LAST_EXECUTION_PROBLEMS: Mutex<Vec<ReportExecutionProblem>> = Mutex::new(Vec::new());

/// Observer handle for coverage tracking
const HANDLE_UCOVERAGE: Cow<'static, str> = Cow::Borrowed("uCoverage");
/// Observer handle for state difference tracking
const HANDLE_STATE_DIFFERENCE: Cow<'static, str> = Cow::Borrowed("stateDifference");

/// Executor for AFL-style fuzzing operations
///
/// This struct manages the execution of code samples and tracks fuzzing state
/// during AFL-style fuzzing operations.
#[allow(dead_code)]
pub struct AFLExecutor<'a, OT, I, S> {
    /// UDP connection to the fuzzing device
    udp: &'a mut DeviceConnection,
    /// Interface to the fuzzing node
    interface: &'a Arc<FuzzerNodeInterface>,
    /// Database for storing results
    database: &'a mut Database,
    /// Observers for tracking execution state
    observers: OT,
    /// Phantom data for type parameters
    phantom: PhantomData<(I, S)>,
    /// Seed for random number generation
    seed: u64,
    /// Last code sample that was executed
    last_code_executed: Option<Code>,
    /// Last reported address exclusion
    last_reported_exclusion: Option<(Option<u16>, u16)>, // address, times
    /// Timeout for fuzzing operations
    timeout_at: Option<Instant>,
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
        seed: u64,
        timeout_hours: Option<u32>,
    ) -> Self {
        AFLExecutor {
            udp,
            interface,
            observers,
            phantom: PhantomData,
            database,
            last_code_executed: None,
            last_reported_exclusion: None,
            seed,
            timeout_at: timeout_hours
                .map(|x| Instant::now() + Duration::from_secs((x as u64) * 60 * 60)),
        }
    }
}

struct ExecuteContext {
    input: Vec<u8>,

    udp: &'static mut DeviceConnection,
    interface: &'static Arc<FuzzerNodeInterface>,
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
        mgr: &mut EM,
        input: &I,
    ) -> Result<ExitKind, libafl::Error> {
        if let Some(at) = self.timeout_at {
            if Instant::now() > at {
                let _ = mgr.send_exiting();
                return Err(libafl::Error::ShuttingDown);
            }
        }

        let first_run = *state.executions() == 0;
        *state.executions_mut() += 1;

        let iteration = *state.executions();

        let mut thread_context = unsafe {
            ExecuteContext {
                input: input.target_bytes().to_vec(),
                udp: transmute::<&mut DeviceConnection, &'static mut DeviceConnection>(
                    &mut *self.udp,
                ),
                interface: transmute::<&Arc<FuzzerNodeInterface>, &'static Arc<FuzzerNodeInterface>>(
                    &*self.interface,
                ),
                database: transmute::<&mut Database, &'static mut Database>(&mut *self.database),
                last_code_executed: transmute::<&mut Option<Code>, &'static mut Option<Code>>(
                    &mut self.last_code_executed,
                ),
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
                if first_run {
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
                        if force_reconnect(
                            &mut thread_context,
                            CommandExitResult::RetryOrReconnect,
                            &mut retry_counter,
                        )
                        .await?
                        {
                            let _ = thread_context.udp.send(OtaC2DTransport::AreYouThere);
                            let result = net_fuzzing_pretext(
                                thread_context.udp,
                                thread_context.database,
                                &mut thread_context.last_code_executed,
                                &mut None,
                            )
                            .await;
                            if matches!(
                                result,
                                CommandExitResult::RetryOrReconnect
                                    | CommandExitResult::ForceReconnect
                            ) {
                                error!(
                                    "Failed to reinitialize the device after rerun: {:?}",
                                    result
                                );
                            }
                        }
                        continue;
                    }
                    ExecuteSampleResult::Success((exit, problems)) => {
                        let _ = force_reconnect(
                            &mut thread_context,
                            CommandExitResult::Operational,
                            &mut retry_counter,
                        )
                        .await?;

                        thread_context.database.push_results(
                            thread_context.input,
                            exit.clone(),
                            problems.clone(),
                            iteration,
                            1234,
                        );

                        if let Err(err) = thread_context.database.save().await {
                            error!("Failed to save the database: {:?}", err);
                        }
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
                    "Error in tokio async task".into(),
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

/// Observer for tracking code coverage during execution
///
/// This struct maintains a map of covered addresses and their hit counts
/// during code execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CoverageObserver {
    /// Map of covered addresses and hit count
    coverage: BTreeMap<u16, u16>,
    /// Raw coverage data slice
    #[serde(skip, default = "default_array")]
    slice: [u8; 0x7c00],
    /// disable feedback
    disable_feedback: bool,
}

fn default_array() -> [u8; 0x7c00] {
    [0; 0x7c00]
}

impl Default for CoverageObserver {
    fn default() -> Self {
        CoverageObserver {
            coverage: BTreeMap::new(),
            slice: [0; 0x7c00],
            disable_feedback: false,
        }
    }
}

impl<I, S> Observer<I, S> for CoverageObserver
where
    S: HasRand,
{
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
        state: &mut S,
        _input: &I,
        _exit_kind: &ExitKind,
    ) -> Result<(), libafl::Error> {
        if let Ok(data) = LAST_EXECUTION_RESULT.lock() {
            if let Some(execution_result) = data.as_ref() {
                if self.disable_feedback {
                    self.coverage.clear();
                    self.coverage
                        .insert(state.rand_mut().between(0, 0x7bff) as u16, 1);
                    for address in 0..0x7c00 {
                        if state.rand_mut().between(0, 999) <= 0 {
                            self.coverage.insert(address, 1);
                        }
                    }
                } else {
                    self.coverage.clone_from(&execution_result.coverage);
                }

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

/// Observer for tracking state differences between executions
///
/// This struct maintains a list of execution problems that indicate
/// differences between normal and serialized execution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DifferenceObserver {
    /// List of execution problems
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

/// Generator for creating input samples from a corpus
///
/// This struct generates input samples by selecting instructions from
/// a predefined corpus.
struct CorpusGenerator {
    /// Corpus of instructions to generate from
    corpus: Vec<CorpusInstruction>,
}

impl CorpusGenerator {
    pub fn new(corpus: Vec<CorpusInstruction>) -> Self {
        CorpusGenerator { corpus }
    }
}

impl<S> Generator<BytesInput, S> for CorpusGenerator
where
    S: HasRand,
{
    fn generate(&mut self, state: &mut S) -> Result<BytesInput, libafl::Error> {
        let mut code = Vec::new();
        while code.len() < 32 {
            let instruction = self
                .corpus
                .get(state.rand_mut().between(0, self.corpus.len() - 1))
                .expect("couldn't get instruction");
            code.extend_from_slice(&instruction.bytes);
        }
        Ok(BytesInput::new(code))
    }
}

/// Main entry point for AFL-style fuzzing
///
/// This function sets up and runs the AFL fuzzing process with the given
/// configuration.
pub async fn afl_main(
    udp: &mut DeviceConnection,
    interface: &Arc<FuzzerNodeInterface>,
    db: &mut Database,
    corpus: Option<PathBuf>,
    initial_corpus: Option<Vec<CorpusInstruction>>,
    solution: Option<PathBuf>,
    timeout_hours: Option<u32>,
    disable_feedback: bool,
    seed: u64,
    printable_input_generation: bool,
) {
    let initial_corpus_gen = initial_corpus.map(|instructions| CorpusGenerator::new(instructions));

    let udp_unsafe_ref =
        unsafe { transmute::<&mut DeviceConnection, &'static mut DeviceConnection>(udp) };
    let interface_ref = Arc::clone(interface);

    for _ in 0..10 {
        match udp_unsafe_ref.send(OtaC2DTransport::AreYouThere).await {
            Ok(_) => break,
            Err(e) => {
                warn!("Waking up device {:?}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    guarantee_initial_state(&interface_ref, udp).await;

    let db_unsafe_ref = unsafe { transmute::<&mut Database, &'static mut Database>(db) };
    let interface_unsafe_ref = unsafe {
        transmute::<&Arc<FuzzerNodeInterface>, &'static Arc<FuzzerNodeInterface>>(interface)
    };

    let result = tokio::task::spawn_blocking(move || {
        let mut coverage_observer = CoverageObserver::default();
        coverage_observer.disable_feedback = disable_feedback;

        let mut generator_rand = RandBytesGenerator::new(nonzero!(32));
        let mut generator_printable = RandPrintablesGenerator::new(nonzero!(32));

        // With coverage feedback
        let mut feedback = MaxMapFeedback::new(&coverage_observer);
        let mut objective = CrashFeedback::new();

        let mut state = StdState::new(
            StdRand::new(),
            CachedOnDiskCorpus::new(corpus.unwrap_or(PathBuf::from("./corpus")), 2000)
                .expect("Corpus initialization failed"),
            OnDiskCorpus::new(solution.unwrap_or(PathBuf::from("./findings")))
                .expect("Corpus initialization failed"),
            &mut feedback,
            &mut objective,
        )
        .unwrap();

        let mon = SimpleMonitor::new(|s| println!("{s}"));
        let mut mgr = SimpleEventManager::new(mon);

        let scheduler = QueueScheduler::new();

        let mut fuzzer = StdFuzzer::new(scheduler, feedback, objective);

        let mut executor = AFLExecutor::new(
            udp_unsafe_ref,
            interface_unsafe_ref,
            db_unsafe_ref,
            tuple_list!(coverage_observer),
            seed,
            timeout_hours,
        );

        if let Some(mut initial) = initial_corpus_gen {
            state
                .generate_initial_inputs(&mut fuzzer, &mut executor, &mut initial, &mut mgr, 8)
                .expect("couldn't generate initial corpus");
        } else {
            if printable_input_generation {
                state
                    .generate_initial_inputs(
                        &mut fuzzer,
                        &mut executor,
                        &mut generator_printable,
                        &mut mgr,
                        8,
                    )
                    .expect("Failed to generate the initial corpus");
            } else {
                state
                    .generate_initial_inputs(
                        &mut fuzzer,
                        &mut executor,
                        &mut generator_rand,
                        &mut mgr,
                        8,
                    )
                    .expect("Failed to generate the initial corpus");
            }
        }

        let mutator = HavocScheduledMutator::new(havoc_mutations());
        let mut stages = tuple_list!(StdMutationalStage::new(mutator));

        fuzzer.fuzz_loop(&mut stages, &mut executor, &mut state, &mut mgr)
    })
    .await;

    info!("Stopped fuzzer with result: {:?}", result);

    if let Err(e) = db.save().await {
        error!("Failed to save the database: {:?}", e);
    }
}
