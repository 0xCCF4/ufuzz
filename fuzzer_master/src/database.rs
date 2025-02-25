use fuzzer_data::genetic_pool::GeneticSampleRating;
use fuzzer_data::{Code, ExecutionResult, ReportExecutionProblem};
use hypervisor::state::{VmExitReason, VmState};
use lazy_static::lazy_static;
use regex::Regex;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
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

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct Database {
    #[serde(serialize_with = "serialize_hex", deserialize_with = "deserialize_hex")]
    pub blacklisted_addresses: BTreeSet<u16>,

    pub results: Vec<CodeResult>,
}

impl Database {
    pub fn from_file<A: AsRef<Path>>(path: A) -> io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let db = serde_json::from_reader(reader)?;
        Ok(db)
    }

    pub fn save<A: AsRef<Path>>(&self, path: A) -> io::Result<()> {
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, self)?;
        Ok(())
    }

    pub fn push_results(
        &mut self,
        code: Code,
        result: ExecutionResult,
        events: Vec<ReportExecutionProblem>,
        evolution: u64,
        seed: u64,
    ) {
        let entry = match self.results.iter_mut().filter(|x| x.code == code).next() {
            Some(x) => x,
            None => {
                self.results.push(CodeResult::default());
                self.results.last_mut().unwrap()
            }
        };

        entry.coverage.extend(result.coverage);
        entry.exit = result.exit;
        entry.state = result.state;
        entry.fitness = result.fitness;

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
}

fn serialize_hex<S>(v: &BTreeSet<u16>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    if serializer.is_human_readable() {
        let mut seq = serializer.serialize_seq(Some(v.len()))?;
        for value in v {
            seq.serialize_element(&format!("{value:04X}"))?;
        }
        seq.end()
    } else {
        v.serialize(serializer)
    }
}

fn deserialize_hex<'de, D>(deserializer: D) -> Result<BTreeSet<u16>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: BTreeSet<String> = BTreeSet::deserialize(deserializer)?;

    let numbers = v.iter().map(|x| {
        let x = x.trim();
        let x = x.trim_start_matches("0x");
        u16::from_str_radix(x, 16).map_err(serde::de::Error::custom)
    });
    let mut result = BTreeSet::new();

    for number in numbers {
        result.insert(number?);
    }

    Ok(result)
}
