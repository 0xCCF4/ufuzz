use fuzzer_data::genetic_pool::GeneticSampleRating;
use fuzzer_data::{Code, ExecutionResult, ReportExecutionProblem};
use hypervisor::state::{VmExitReason, VmState};
use itertools::Itertools;
use lazy_static::lazy_static;
use log::error;
use performance_timing::measurements::{MeasureValues, MeasurementCollection, MeasurementData};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io;

lazy_static! {
    pub static ref NUMBER_REGEX: Regex = Regex::new(r" +[0-9]+,").unwrap();
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CodeEvent {
    CoverageProblem {
        address: u16,
        coverage_exit: Option<VmExitReason>,
        coverage_state: Option<VmState>,
    },
    SerializedMismatch {
        code: Code,
        serialized_exit: Option<VmExitReason>,
        serialized_state: Option<VmState>,
    },
    StateTraceMismatch {
        code: Code,
        index: u64,
        normal: Option<VmState>,
        serialized: Option<VmState>,
    },
    VeryLikelyBug {
        code: Code,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct FoundAt {
    pub seed: u64,
    pub evolution: u64,
}

impl Ord for FoundAt {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let r = self.seed.cmp(&other.seed);
        if r == Ordering::Equal {
            self.evolution.cmp(&other.evolution)
        } else {
            r
        }
    }
}

impl PartialOrd for FoundAt {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CodeResult {
    pub code: Code,
    pub coverage: BTreeMap<u16, u16>,
    pub exit: VmExitReason,
    pub state: VmState,
    pub fitness: GeneticSampleRating,
    pub events: Vec<CodeEvent>,
    pub found_at: BTreeSet<FoundAt>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ExcludeType {
    Normal,
    VMentry,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BlacklistEntry {
    pub address: u16,
    pub iteration: u32,
    pub code: Code,
    pub exclude_type: ExcludeType,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct DatabaseData {
    pub blacklisted_addresses: Vec<BlacklistEntry>,

    pub results: Vec<CodeResult>,
    pub performance: MeasurementCollection<u64>,
}

pub struct Database {
    pub path: PathBuf,
    pub data: DatabaseData,
    pub save_mutex: Arc<tokio::sync::Mutex<()>>,
    pub dirty: bool,
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Database {
            path: self.path.clone(),
            data: self.data.clone(),
            save_mutex: Arc::clone(&self.save_mutex),
            dirty: self.dirty,
        }
    }
}

impl Database {
    pub fn from_file<A: AsRef<Path>>(path: A) -> io::Result<Self> {
        let file = std::fs::File::open(&path)?;
        let reader = std::io::BufReader::new(file);
        let db = serde_json::from_reader(reader)?;

        let mut result = Database {
            path: path.as_ref().to_path_buf(),
            data: db,
            save_mutex: Arc::new(tokio::sync::Mutex::new(())),
            dirty: false,
        };
        result
            .data
            .performance
            .data
            .push(MeasurementData::default());

        Ok(result)
    }

    pub fn empty<A: AsRef<Path>>(path: A) -> Self {
        let mut result = Self {
            path: path.as_ref().to_path_buf(),
            data: DatabaseData::default(),
            save_mutex: Arc::new(tokio::sync::Mutex::new(())),
            dirty: false,
        };
        result
            .data
            .performance
            .data
            .push(MeasurementData::default());
        result
    }

    pub async fn save(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }

        let _guard = self.save_mutex.lock().await;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.path.with_extension("new"))
            .map_err(|e| {
                let msg = format!(
                    "Failed to create or open new file at {}: {}",
                    self.path.display(),
                    e
                );
                error!("{}", msg);
                e
            })?;
        let mut writer = std::io::BufWriter::new(file);
        serde_json::to_writer(&mut writer, &self.data).map_err(|e| {
            let msg = format!(
                "Failed to write data to file at {}: {}",
                self.path.display(),
                e
            );
            error!("{}", msg);
            e
        })?;
        drop(writer);

        if self.path.exists() {
            let _ = std::fs::remove_file(&self.path).map_err(|e| {
                let msg = format!(
                    "Failed to remove old file at {}: {}",
                    self.path.display(),
                    e
                );
                error!("{}", msg);
                e
            })?;
        }
        std::fs::rename(self.path.with_extension("new"), &self.path).map_err(|e| {
            let msg = format!(
                "Failed to rename new file to original at {}: {}",
                self.path.display(),
                e
            );
            error!("{}", msg);
            e
        })?;
        self.dirty = false;

        Ok(())
    }

    pub fn mark_dirty(&mut self) {
        self.update_perf_values();
        self.dirty = true;
    }

    pub fn exclude_address(&mut self, address: u16, code: Code, exclude_type: ExcludeType) {
        let highest_exclusion_iteration = self
            .data
            .blacklisted_addresses
            .iter()
            .map(|v| v.iteration)
            .max()
            .unwrap_or_default();
        let iteration = highest_exclusion_iteration.saturating_add(1);

        if self.blacklisted().contains(&address) {
            error!("Address: {address:04x} is already excluded");
            return;
        }

        self.mark_dirty();

        self.data.blacklisted_addresses.push(BlacklistEntry {
            address,
            iteration,
            code,
            exclude_type,
        });
    }

    pub fn blacklisted<'a>(&'a self) -> Box<dyn Iterator<Item = u16> + 'a> {
        Box::new(
            self.data
                .blacklisted_addresses
                .iter()
                .map(|v| v.address)
                .unique(),
        )
    }

    pub fn push_results(
        &mut self,
        code: Code,
        result: ExecutionResult,
        events: Vec<ReportExecutionProblem>,
        evolution: u64,
        seed: u64,
    ) {
        let entry = match self
            .data
            .results
            .iter_mut()
            .filter(|x| x.code == code)
            .next()
        {
            Some(x) => x,
            None => {
                self.data.results.push(CodeResult::default());
                self.data.results.last_mut().unwrap()
            }
        };

        self.dirty = true;

        entry.coverage.extend(result.coverage);
        entry.exit = result.exit;
        entry.state = result.state;
        entry.fitness = result.fitness;
        entry.code = code;

        entry.found_at.insert(FoundAt { seed, evolution });

        for event in events {
            match event {
                ReportExecutionProblem::CoverageProblem {
                    address,
                    coverage_exit,
                    coverage_state,
                } => {
                    entry.events.push(CodeEvent::CoverageProblem {
                        address,
                        coverage_exit,
                        coverage_state,
                    });
                }
                ReportExecutionProblem::SerializedMismatch {
                    serialized_exit,
                    serialized_state,
                } => {
                    entry.events.push(CodeEvent::SerializedMismatch {
                        code: result.serialized.clone().unwrap_or_default(),
                        serialized_exit,
                        serialized_state,
                    });
                }
                ReportExecutionProblem::StateTraceMismatch {
                    index,
                    normal,
                    serialized,
                } => {
                    entry.events.push(CodeEvent::StateTraceMismatch {
                        code: result.serialized.clone().unwrap_or_default(),
                        index,
                        normal,
                        serialized,
                    });
                }
                ReportExecutionProblem::VeryLikelyBug => {
                    entry.events.push(CodeEvent::VeryLikelyBug {
                        code: result.serialized.clone().unwrap_or_default(),
                    });
                }
            }
        }
    }

    fn update_perf_values(&mut self) {
        let measurements = performance_timing::measurements::mm_instance()
            .borrow()
            .data
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect::<BTreeMap<String, MeasureValues<u64>>>();

        let last = self.data.performance.data.last_mut().unwrap();
        last.clone_from(&measurements);
    }
}
