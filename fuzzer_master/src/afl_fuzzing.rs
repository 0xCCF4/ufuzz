use std::borrow::Cow;
use std::collections::BTreeMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::Deref;
use crate::database::Database;
use crate::device_connection::DeviceConnection;
use crate::fuzzer_node_bridge::FuzzerNodeInterface;
use libafl::corpus::{InMemoryCorpus, OnDiskCorpus};
use libafl::events::{SendExiting, SimpleEventManager};
use libafl::executors::{Executor, ExitKind, HasObservers};
use libafl::feedbacks::{CrashFeedback, MaxMapFeedback};
use libafl::generators::RandPrintablesGenerator;
use libafl::monitors::SimpleMonitor;
use libafl::mutators::{havoc_mutations, HavocScheduledMutator};
use libafl::observers::{MapObserver, Observer, ObserversTuple};
use libafl::schedulers::QueueScheduler;
use libafl::stages::StdMutationalStage;
use libafl::state::{HasExecutions, StdState};
use libafl::{Fuzzer, StdFuzzer};
use libafl_bolts::rands::StdRand;
use libafl_bolts::tuples::{tuple_list, HasConstLen, MatchNameRef, RefIndexable};
use libafl_bolts::{nonzero, Error, ErrorBacktrace, HasLen, Named};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use libafl::inputs::{HasTargetBytes, InputToBytes};
use log::error;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;
use fuzzer_data::{Code, ExecutionResult, ReportExecutionProblem};
use crate::CommandExitResult;
use crate::net::{net_execute_sample, net_fuzzing_pretext, ExecuteSampleResult};


static LAST_EXECUTION_RESULT: Mutex<Option<ExecutionResult>> = Mutex::new(None);
static LAST_EXECUTION_PROBLEMS: Mutex<Vec<ReportExecutionProblem>> = Mutex::new(Vec::new());

const HANDLE_UCOVERAGE: Cow<'static, str> = Cow::Borrowed("uCoverage");
const HANDLE_STATE_DIFFERENCE: Cow<'static, str> = Cow::Borrowed("stateDifference");


pub struct AFLExecutor<'a, OT, I, S> where OT: ObserversTuple<I, S> {
    udp: &'a mut DeviceConnection,
    interface: &'a Arc<FuzzerNodeInterface>,
    database: &'a mut Database,
    observers: OT,
    phantom: PhantomData<(I, S)>,
    runtime: Runtime,
    
    last_code_executed: Option<Code>,
    last_reported_exclusion: Option<(Option<u16>, u16)>, // address, times
}

impl<'a, OT, I, S> AFLExecutor<'a, OT, I, S> where OT: ObserversTuple<I, S> {
    fn new(udp: &'a mut DeviceConnection, interface: &'a Arc<FuzzerNodeInterface>, database: &mut Database, observers: OT) -> Self {
        AFLExecutor {
            udp,
            interface,
            observers,
            phantom: PhantomData,
            runtime: tokio::runtime::Runtime::new().expect("create tokio runtime"),
            database,
            last_code_executed: None,
            last_reported_exclusion: None,
        }
    }
}

impl<'a, EM, I, S, Z, OT> Executor<EM, I, S, Z> for AFLExecutor<'a, OT, I, S> where
    S: HasExecutions, OT: ObserversTuple<I, S>, EM: SendExiting, I: HasTargetBytes {
    fn run_target(&mut self, fuzzer: &mut Z, state: &mut S, mgr: &mut EM, input: &I) -> Result<ExitKind, Error> {
        *state.executions_mut() += 1;

        let _guard = self.runtime.enter();
        let task = tokio::spawn(async {
            async fn force_reconnect() {
                
            }
            
            let mut retry_counter = 0u64;
            loop {
                let result = net_fuzzing_pretext(
                    self.udp,
                    self.database,
                    &mut self.last_code_executed,
                    &mut self.last_reported_exclusion,
                ).await;
                
                match result {
                    CommandExitResult::ExitProgram => {
                        error!("Exiting program as requested net_fuzzing_pretext");
                        let _ = mgr.send_exiting();
                        return Result::Err(libafl::Error::Runtime("stop".into(), ErrorBacktrace::new()));
                    }
                    CommandExitResult::Operational => {},
                    CommandExitResult::RetryOrReconnect => {
                        retry_counter += 1;
                        continue;
                    }
                    CommandExitResult::ForceReconnect => {
                        force_reconnect().await;
                        continue;
                    }
                }

                self.last_code_executed = Some(input.target_bytes().to_vec());
                
                let result = net_execute_sample(
                    self.udp,
                    self.interface,
                    self.database,
                    input.target_bytes().as_ref(),
                ).await;

                match net_execute_sample(self.udp, self.interface, self.database, input.target_bytes().as_ref()).await {
                    ExecuteSampleResult::Timeout => { 
                        force_reconnect().await;
                        continue;
                    }
                    ExecuteSampleResult::Rerun => { 
                        retry_counter += 1;
                        continue;
                    },
                    ExecuteSampleResult::Success((exit, problems)) => { 
                        return Result::Ok((exit, problems));
                    },
                };
            }
        });
        let result = self.runtime.block_on(task);

        match result {
            Result::Err(e) => {
                error!("Error executing target: {:?}", e);
                return Err(libafl::Error::Runtime("Error in tokio asyn task".into(), ErrorBacktrace::new()));
            }
            Result::Ok(result) => {
                let (exit, problems) = result?;
                if let Ok(mut data) = LAST_EXECUTION_RESULT.lock() {
                    *data = Some(exit.clone());
                }
                if let Ok(mut data) = LAST_EXECUTION_PROBLEMS.lock() {
                    data.clear();
                    data.extend(problems);
                }
                if exit.exit // todo
            }
        }

        Ok(ExitKind::Ok)
    }
}

impl<'a, OT, I, S> HasObservers for AFLExecutor<'a, OT, I, S> where OT: ObserversTuple<I, S> {
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
    #[serde(skip, default="default_array")]
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

impl<I, S> Observer<I, S> for CoverageObserver  {
    fn pre_exec(&mut self, _state: &mut S, _input: &I) -> Result<(), Error> {
        if let Ok(mut data) = LAST_EXECUTION_RESULT.lock() {
            if data.is_some() {
                *data = None;
            }
        }
        self.reset_map()?;
        Ok(())
    }
    fn post_exec(&mut self, _state: &mut S, _input: &I, _exit_kind: &ExitKind) -> Result<(), Error> {
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
        self.coverage.get(&(idx as u16)).unwrap_or(&0).clone().max(u8::MAX as u16) as u8
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

    fn reset_map(&mut self) -> Result<(), Error> {
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
    problems: Vec<ReportExecutionProblem>
}

impl<I, S> Observer<I, S> for DifferenceObserver {
    fn pre_exec(&mut self, _state: &mut S, _input: &I) -> Result<(), Error> {
        if let Ok(mut data) = LAST_EXECUTION_PROBLEMS.lock() {
            data.clear();
        }
        Ok(())
    }

    fn post_exec(&mut self, _state: &mut S, _input: &I, _exit_kind: &ExitKind) -> Result<(), Error> {
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
    _db: &mut Database,
    corpus: Option<PathBuf>,
    solution: Option<PathBuf>,
    timeout_hours: Option<u32>,
    disable_feedback: bool,
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
        OnDiskCorpus::new(solution.unwrap_or(PathBuf::from("./findings"))).expect("Corpus initialization failed"),
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
    let mut executor = AFLExecutor::new(udp, interface, tuple_list!(coverage_observer));

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
