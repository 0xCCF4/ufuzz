//! Database Module
//!
//! This module provides functionality for storing and managing fuzzing results and
//! performance data.

use fuzzer_data::genetic_pool::GeneticSampleRating;
use fuzzer_data::{Code, ExecutionResult, ReportExecutionProblem};
use hypervisor::state::{VmExitReason, VmState};
use itertools::Itertools;
use lazy_static::lazy_static;
use log::error;
use performance_timing::measurements::{MeasureValues, MeasurementCollection, MeasurementData};
use performance_timing::track_time;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io;

lazy_static! {
    /// Regular expression for matching numbers in text
    pub static ref NUMBER_REGEX: Regex = Regex::new(r" +[0-9]+,").unwrap();
}

/// Events that can occur during code execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CodeEvent {
    /// Coverage collection problem
    CoverageProblem {
        /// Address where problem occurred
        address: u16,
        /// Exit reason from coverage collection, set if the exit reason is different from the normal exit reason
        coverage_exit: Option<VmExitReason>,
        /// State during coverage collection, set if the state is different from the normal state
        coverage_state: Option<VmState>,
    },
    /// Mismatch between serialized and normal execution
    SerializedMismatch {
        /// Code that caused the mismatch
        code: Code,
        /// Exit reason from serialized execution, set if the exit reason is different from the normal exit reason
        serialized_exit: Option<VmExitReason>,
        /// State from serialized execution, set if the state is different from the normal state
        serialized_state: Option<VmState>,
    },
    /// Mismatch in state trace
    StateTraceMismatch {
        /// Code that caused the mismatch
        code: Code,
        /// Index where mismatch occurred
        index: u64,
        /// State from normal execution, set if the state is different from the normal state
        normal: Option<VmState>,
        /// State from serialized execution, set if the state is different from the normal state
        serialized: Option<VmState>,
    },
    /// Indicates a likely bug in the code
    VeryLikelyBug {
        /// Code that triggered the bug
        code: Code,
    },
    /// Access to coverage collection area
    AccessCoverageArea,
}

/// Information about due to which fuzzing run a code sample was found
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct FoundAt {
    /// Seed value when found
    pub seed: u64,
    /// Evolution number when found
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

/// Results of executing a code sample
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct CodeResult {
    /// The code sample
    pub code: Code,
    /// Coverage information for each address
    pub coverage: BTreeMap<u16, u16>,
    /// Exit reason from execution
    pub exit: VmExitReason,
    /// Final VM state
    pub state: VmState,
    /// Fitness rating for genetic algorithms
    pub fitness: GeneticSampleRating,
    /// Events that occurred during execution
    pub events: Vec<CodeEvent>,
    /// When this code was found
    pub found_at: BTreeSet<FoundAt>,
    /// Timestamps when this code was found
    #[serde(default)]
    pub found_on: Vec<Timestamp>,
}

/// Timestamp for tracking when events occur
#[derive(Debug, Serialize, Deserialize, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
#[repr(transparent)]
pub struct Timestamp(u64);

impl Timestamp {
    /// Zero timestamp
    pub const ZERO: Timestamp = Timestamp(0);

    /// Creates a new timestamp for the current time
    pub fn now() -> Self {
        Timestamp(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        )
    }

    /// Converts to a system time
    pub fn instant(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(self.0)
    }

    /// Returns the timestamp in seconds
    pub fn seconds(&self) -> u64 {
        self.0
    }
}

/// Type of address exclusion
#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ExcludeType {
    /// Normal exclusion due to coverage collection
    Normal,
    /// VM entry point exclusion
    VMentry,
}

/// Entry in the blacklist of excluded addresses
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BlacklistEntry {
    /// Address to exclude
    pub address: u16,
    /// Iteration when excluded
    pub iteration: u32,
    /// Code that triggered the exclusion
    pub code: Code,
    /// Type of exclusion
    pub exclude_type: ExcludeType,
}

/// Core data structure for the database
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct DatabaseData {
    /// List of blacklisted addresses
    pub blacklisted_addresses: Vec<BlacklistEntry>,
    /// Results of code execution
    pub results: Vec<CodeResult>,
    /// Performance measurements
    #[serde(default)]
    pub performance: MeasurementCollection<u64>,
    /// Device performance measurements
    #[serde(default)]
    pub device_performance: MeasurementCollection<f64>,
}

impl DatabaseData {
    /// Merges another database's data into this one
    pub fn merge(&mut self, other: DatabaseData) {
        self.blacklisted_addresses
            .extend(other.blacklisted_addresses);
        self.results.extend(other.results);
        self.performance.data.extend(other.performance.data);
        self.device_performance
            .data
            .extend(other.device_performance.data);
    }
}

/// Main database structure for managing fuzzing data
pub struct Database {
    /// Path to the database file
    pub path: PathBuf,
    /// Database contents
    pub data: DatabaseData,
    /// Mutex for synchronizing saves
    pub save_mutex: Arc<tokio::sync::Mutex<()>>,
    /// Whether the database has unsaved changes
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
    /// Loads a database from a file
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the database file
    ///
    /// # Returns
    ///
    /// * `io::Result<Self>` - New database or error
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

    /// Creates a new empty database
    ///
    /// # Arguments
    ///
    /// * `path` - Path for the new database file
    ///
    /// # Returns
    ///
    /// * `Self` - New empty database
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

    /// Merges another database into this one
    ///
    /// # Arguments
    ///
    /// * `other` - Database to merge
    pub fn merge(&mut self, other: Self) {
        self.data.merge(other.data);
        self.mark_dirty();
    }

    /// Saves the database to disk
    ///
    /// # Returns
    ///
    /// * `io::Result<()>` - Success or error
    #[track_time("host::database::save")]
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

    /// Marks the database as having unsaved changes
    pub fn mark_dirty(&mut self) {
        self.update_perf_values();
        self.dirty = true;
    }

    /// Adds an address to the blacklist
    ///
    /// # Arguments
    ///
    /// * `address` - Address to blacklist
    /// * `code` - Code that triggered the blacklist
    /// * `exclude_type` - Type of exclusion
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

    /// Returns an iterator over blacklisted addresses
    pub fn blacklisted<'a>(&'a self) -> Box<dyn Iterator<Item = u16> + 'a> {
        Box::new(
            self.data
                .blacklisted_addresses
                .iter()
                .map(|v| v.address)
                .unique(),
        )
    }

    /// Updates device performance measurements
    ///
    /// # Arguments
    ///
    /// * `perf` - New performance measurements
    pub fn set_device_performance(&mut self, perf: MeasurementCollection<f64>) {
        self.data.device_performance = perf;
        self.mark_dirty();
    }

    /// Adds execution results to the database
    ///
    /// # Arguments
    ///
    /// * `code` - Code that was executed
    /// * `result` - Execution results
    /// * `events` - Events that occurred
    /// * `evolution` - Evolution number
    /// * `seed` - Seed value
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
        entry.found_on.push(Timestamp::now());

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
                ReportExecutionProblem::AccessCoverageArea => {
                    entry.events.push(CodeEvent::AccessCoverageArea);
                }
            }
        }
    }

    /// Updates performance measurements
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
