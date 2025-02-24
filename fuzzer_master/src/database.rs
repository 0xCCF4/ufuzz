use fuzzer_data::{ReportExecutionProblem, ReportExecutionProblemType};
use lazy_static::lazy_static;
use regex::Regex;
use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use tokio::io;

lazy_static! {
    pub static ref NUMBER_REGEX: Regex = Regex::new(r" +[0-9]+,").unwrap();
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Database {
    #[serde(serialize_with = "serialize_hex", deserialize_with = "deserialize_hex")]
    pub blacklisted_addresses: BTreeSet<u16>,
    pub events: Vec<ReportExecutionProblem>,
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

    pub fn push_event(&mut self, event: ReportExecutionProblem) {
        if self.events.contains(&event) {
            return;
        }
        self.events.push(event);
    }

    pub fn get_blacklisted_because_coverage_problem(&self) -> BTreeSet<u16> {
        self.events
            .iter()
            .filter_map(|x| match x.event {
                ReportExecutionProblemType::CoverageProblem { ref addresses } => Some(addresses),
                _ => None,
            })
            .flat_map(|x| x.iter().cloned())
            .collect()
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
