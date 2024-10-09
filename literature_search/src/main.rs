use std::cmp::PartialEq;
use std::collections::{HashSet};
use error_chain::error_chain;
use serde::{Deserialize, Serialize};
use std::env;
use std::io::Write;
use std::ops::{Add, Deref, DerefMut};
use crossterm::event;
use crossterm::event::{KeyCode, KeyEventKind};
use ratatui::DefaultTerminal;
use ratatui::layout::{Alignment};
use ratatui::style::Stylize;
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::block::{Position, Title};
use ratatui::widgets::{Block, Paragraph, Widget, Wrap};
use regex::{Regex, RegexBuilder};
use reqwest::{Client, Method, RequestBuilder};
use serde::de::DeserializeOwned;
use tokio::sync::{Mutex};
use crate::IncludedPaperStatus::CoreLiterature;

error_chain! {
    foreign_links {
        EnvVar(env::VarError);
        HttpRequest(reqwest::Error);
        IoError(std::io::Error);
        SerdeError(serde_json::Error);
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AuthorWeak {
    #[serde(alias = "authorId")]
    author_id: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
pub struct Tldr {
    model: Option<String>,
    text: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct RelevancePaperWeak {
    #[serde(alias = "paperId")]
    paper_id: Option<String>,
    title: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct RelevancePaper {
    #[serde(flatten)]
    paper: RelevancePaperWeak,
    year: Option<u32>,
    url: Option<String>,
    #[serde(alias = "abstract")]
    abstract_text: Option<String>,
    authors: Vec<AuthorWeak>,
    #[serde(alias = "referenceCount")]
    reference_count: u32,
    #[serde(alias = "citationCount")]
    citation_count: u32,
    #[serde(alias = "influentialCitationCount")]
    influential_citation_count: u32,
    citations: Vec<RelevancePaperWeak>,
    references: Vec<RelevancePaperWeak>,
    tldr: Option<Tldr>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct RelevanceResponse {
    total: u32,
    offset: u32,
    next: u32,
    data: Vec<RelevancePaper>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Copy, PartialEq, Eq)]
pub enum IncludedPaperStatus {
    CoreLiterature,
    SideInformation,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct IncludedPaper {
    paper: RelevancePaperWeak,
    status: IncludedPaperStatus,
    message: Option<String>,
}


lazy_static::lazy_static! {
    static ref SEMANTIC_SCHOLAR_API_KEY: Mutex<Option<String>> = Mutex::new(None);
}

async fn query_api_raw<Response>(url: &str, method: Method) -> Result<Response>
where
    Response: DeserializeOwned
{
    async fn build_query(method: Method, url: &str) -> RequestBuilder {
        let query = Client::new()
            .request(method, format!("https://api.semanticscholar.org/{}", url));

        if let Some(key) = SEMANTIC_SCHOLAR_API_KEY.lock().await.deref() {
            query.header("x-api-key", key)
        } else {
            query
        }
    }

    let mut wait_time: u32 = 1;

    loop {
        let now = std::time::Instant::now();
        let response = build_query(method.clone(), url).await.send().await?;
        let elapsed = now.elapsed().as_secs_f32();

        if elapsed > wait_time as f32 {
            wait_time = 1;
        }

        std::io::stdout().flush()?;

        if response.status().is_success() {
            return Ok(response.json().await?);
        } else if response.status().as_u16() == 429 /* too many requests */ {
            tokio::time::sleep(tokio::time::Duration::from_secs(wait_time as u64)).await;
            wait_time *= 2;
            wait_time = wait_time.min(20);
            if rand::random::<f32>() < 0.75f32 {
                print!(".");
            } else {
                print!("|");
            }
        } else if response.status().as_u16() == 404 {
            return Err("Not found".into());
        } else {
            println!("{}", response.status().as_u16());
            println!("Error: {:?}", response);
            tokio::time::sleep(tokio::time::Duration::from_secs(wait_time as u64)).await;
            wait_time *= 2;
            wait_time = wait_time.min(40);
        }
        std::io::stdout().flush()?;
    }
}

async fn query_paper_relevance_raw(query: &str, offset: u32, limit: u32) -> Result<RelevanceResponse> {
    let fields = "paperId,title,url,year,abstract,authors,citations,references,tldr,referenceCount,citationCount,influentialCitationCount";
    let url = format!("graph/v1/paper/search?fields={}&query={}&offset={}&limit={}", fields, query, offset, limit);
    query_api_raw(&url, Method::GET).await
}

async fn query_paper_relevance(query: &str) -> Result<Vec<RelevancePaper>> {
    let mut papers = Vec::new();
    let mut offset = 0;
    let limit = 10;
    loop {
        let response = query_paper_relevance_raw(query, offset, limit).await?;
        println!("OK {}/{}", response.next, response.total);
        papers.extend(response.data);
        if response.next == 0 {
            break;
        }
        offset += response.next;
        if offset >= response.total {
            break;
        }
    }
    Ok(papers)
}

async fn query_paper_data<Id: AsRef<str>>(paper_id: Id) -> Result<RelevancePaper> {
    let fields = "paperId,title,url,year,abstract,authors,citations,references,tldr,referenceCount,citationCount,influentialCitationCount";
    let url = format!("graph/v1/paper/{paper_id}?fields={fields}", paper_id = paper_id.as_ref());

    query_api_raw(&url, Method::GET).await
}

fn read_paper_database() -> Result<Vec<RelevancePaper>> {
    let data = std::fs::read_to_string("database_papers.json")?;
    let papers: Vec<RelevancePaper> = serde_json::from_str(&data)?;
    Ok(papers)
}

#[allow(dead_code)]
async fn keyword_search_all() -> Result<()> {
    // read keywords file
    let file = std::fs::read_to_string("keywords.txt")?;
    let keywords: Vec<&str> = file.lines().collect();

    let mut papers = Vec::new();
    let mut paper_ids = HashSet::new();

    fn insert_paper(paper: RelevancePaper, papers: &mut Vec<RelevancePaper>, paper_ids: &mut HashSet<String>) {
        if !paper_ids.contains(&paper.paper.paper_id.clone().unwrap()) {
            paper_ids.insert(paper.paper.paper_id.clone().unwrap());
            papers.push(paper);
        }
    }

    if let Ok(data) = read_paper_database() {
        data.into_iter().for_each(|paper| insert_paper(paper, &mut papers, &mut paper_ids));
    }

    for (index, keyword) in keywords.iter().enumerate() {
        println!("{}/{}: {}", index, keywords.len(), keyword);
        let papers_queried = query_paper_relevance(keyword).await?;
        papers_queried.into_iter().for_each(|paper| insert_paper(paper, &mut papers, &mut paper_ids));
        let data = serde_json::to_string_pretty(&papers)?;
        std::fs::write("database_papers.json", data)?;
    }

    let data = serde_json::to_string_pretty(&papers)?;
    std::fs::write("database_papers.json", data)?;

    Ok(())
}

#[allow(dead_code)]
fn sifting(papers: &Vec<RelevancePaper>, term: &mut DefaultTerminal) -> Result<bool> {
    let mut ok_papers = std::fs::read_to_string("ok_papers.json")
        .map(|data| serde_json::from_str::<Vec<IncludedPaper>>(&data).unwrap_or_default())
        .unwrap_or_default();
    let mut excluded_papers = std::fs::read_to_string("excluded_papers.json")
        .map(|data| serde_json::from_str::<Vec<RelevancePaperWeak>>(&data).unwrap_or_default())
        .unwrap_or_default();

    let binding = std::fs::read_to_string("highlight.txt").unwrap_or_default();
    let highlights: Vec<&str> = binding.lines().collect();
    let highlights = "(".to_string() + &*highlights.join(")|(") + ")";
    let highlights = RegexBuilder::new(&highlights).case_insensitive(true).build().expect("Regex highlight compile error");

    enum PaperStatus {
        None,
        Excluded,
        Core,
        Side,
    }

    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    enum Mode {
        Sifting,
        Comment,
    }

    let mut index = 0;
    let mut mode = Mode::Sifting;

    loop {
        let paper = papers.get(index);

        term.draw(|frame| {
            let title = Title::from(" Literature sifting ".bold());
            let instructions =
                match mode {
                    Mode::Sifting => Title::from(Line::from(vec![
                        " Back ".into(),
                        "<LT>".blue().bold(),
                        " Forward ".into(),
                        "<RT>".blue().bold(),
                        " Side ".into(),
                        "<Ent/S>".blue().bold(),
                        " Core ".into(),
                        "<C>".blue().bold(),
                        " Exclude ".into(),
                        "<Return>".blue().bold(),
                        " Untaint ".into(),
                        "<Del>".blue().bold(),
                        " Next ".into(),
                        "<Space>".blue().bold(),
                        " Quit ".into(),
                        "<Q> ".blue().bold(),
                        " Exit ".into(),
                        "<E> ".blue().bold(),
                        " Repaint ".into(),
                        "<R> ".blue().bold(),
                        " Msg ".into(),
                        "<M> ".blue().bold(),
                    ])),
                    Mode::Comment => Title::from(Line::from(vec![
                        " Finish ".into(),
                        "<Return/ESC>".blue().bold(),
                    ])),
                };
            let block = Block::bordered()
                .title(title.alignment(Alignment::Center))
                .title(
                    instructions
                        .alignment(Alignment::Center)
                        .position(Position::Bottom),
                )
                .border_set(border::THICK);

            if paper.is_none() {
                index = papers.len();
            }

            let counter_text = match paper {
                None => vec![Line::from("No more papers")],
                Some(paper) => {
                    let amount = papers.len();
                    let mut comment = None;
                    let status = if let Some(paper) = ok_papers.iter().find(|p| p.paper.paper_id == paper.paper.paper_id) {
                        comment = paper.message.clone();
                        match paper.status {
                            IncludedPaperStatus::CoreLiterature => PaperStatus::Core,
                            IncludedPaperStatus::SideInformation => PaperStatus::Side,
                        }
                    } else if excluded_papers.iter().any(|p| p.paper_id == paper.paper.paper_id) {
                        PaperStatus::Excluded
                    } else {
                        PaperStatus::None
                    };
                    let mut status = match status {
                        PaperStatus::None => Line::from(""),
                        PaperStatus::Excluded => Line::from("EXCLUDED").red().bold().underlined(),
                        PaperStatus::Core => Line::from("CORE").green().bold().underlined(),
                        PaperStatus::Side => Line::from("SIDE").yellow().bold().underlined(),
                    };
                    status.extend(Line::from(comment.map(|x| ": ".to_string()+&x).unwrap_or("".to_string())));
                    fn highlight<'a, 'b>(regex: &'a Regex, text: String) -> Vec<Span<'b>> {
                        let mut result = Vec::new();

                        let mut index = 0;

                        for capture in regex.captures_iter(&text) {
                            if let Some(group) = capture.get(0) {
                                let start = group.start();
                                let end = group.end();

                                if start > index {
                                    result.push(Span::from(String::from(&text[index..start])));
                                    index = start;
                                }

                                result.push(Span::from(String::from(&text[start..end])).red());
                                index = end;
                            }
                        }

                        if text.len() > index {
                            result.push(Span::from(String::from(&text[index..])));
                        }

                        result
                    }
                    vec![
                        status,
                        "".into(),
                        format!("Paper ({index}/{amount})").into(),
                        "".into(),
                        Line::from_iter(vec![Span::from("Title").bold().gray().underlined()].into_iter().chain(highlight(&highlights, format!(": {}", paper.paper.title.clone().unwrap_or("<no title>".into())).into()))),
                        "".into(),
                        Line::from_iter(vec![Span::from("TLDR").bold().gray().underlined()].into_iter().chain(highlight(&highlights, format!(": {}", paper.tldr.clone().unwrap_or_default().text.unwrap_or("<no tldr>".into())).into()))),
                        "".into(),
                        Line::from_iter(vec![Span::from("URL").bold().gray().underlined()].into_iter().chain(highlight(&highlights, format!(": {}", paper.url.clone().unwrap_or("<no url>".into())).into()))),
                        "".into(),
                        Line::from_iter(vec![Span::from("Abstract").bold().gray().underlined()].into_iter().chain(highlight(&highlights, format!(": {}", paper.abstract_text.clone().unwrap_or("<no abstract>".into())).into()))),
                    ]
                }
            };

            Paragraph::new(counter_text)
                .left_aligned()
                .wrap(Wrap {trim:false})
                .block(block)
                .render(frame.area(), frame.buffer_mut());
        })?;

        if let event::Event::Key(key) = event::read()? {
            if mode == Mode::Sifting && key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                return Ok(papers.iter().all(|p| ok_papers.iter().any(|op| &op.paper == &p.paper) || excluded_papers.iter().any(|op| op == &p.paper)));
            }
            if mode == Mode::Sifting && key.kind == KeyEventKind::Press && key.code == KeyCode::Char('e') {
                return Ok(false);
            }
            if mode == Mode::Sifting && key.kind == KeyEventKind::Press && key.code == KeyCode::Left {
                loop {
                    if index < 1 {
                        break;
                    }

                    index -= 1;

                    if !key.modifiers.contains(event::KeyModifiers::SHIFT) {
                        break;
                    }

                    if index >= papers.len() {
                        break;
                    }

                    let paper = papers.get(index).unwrap();

                    if ok_papers.iter().any(|p| p.paper.paper_id == paper.paper.paper_id){
                        break;
                    }
                }
            }
            if mode == Mode::Sifting && key.kind == KeyEventKind::Press && key.code == KeyCode::Right {
                loop {
                    index += 1;

                    if !key.modifiers.contains(event::KeyModifiers::SHIFT) {
                        break;
                    }

                    if index >= papers.len() {
                        break;
                    }

                    let paper = papers.get(index).unwrap();

                    if ok_papers.iter().any(|p| p.paper.paper_id == paper.paper.paper_id){
                        break;
                    }
                }
            }
            if mode == Mode::Sifting && key.kind == KeyEventKind::Press && key.code == KeyCode::Char(' ') {
                loop {
                    index += 1;

                    if index >= papers.len() {
                        break;
                    }

                    let paper = papers.get(index).unwrap();

                    if !ok_papers.iter().any(|p| p.paper.paper_id == paper.paper.paper_id) && !excluded_papers.iter().any(|p| p.paper_id == paper.paper.paper_id) {
                        break;
                    }
                }
            }
            let mut save = false;
            let excluded_paper_len = excluded_papers.len();
            let ok_paper_len = ok_papers.len();

            if mode == Mode::Sifting && key.kind == KeyEventKind::Press && (key.code == KeyCode::Enter || key.code == KeyCode::Char('s')) {
                if let Some(paper) = papers.get(index) {
                    ok_papers.retain(|p| p.paper.paper_id.is_none() || p.paper.paper_id != paper.paper.paper_id);
                    ok_papers.push(IncludedPaper {
                        paper: paper.paper.clone(),
                        status: IncludedPaperStatus::SideInformation,
                        message: None,
                    });
                    excluded_papers.retain(|p| p.paper_id.is_none() || p.paper_id != paper.paper.paper_id);
                    save = true;
                }
            }
            if mode == Mode::Sifting && key.kind == KeyEventKind::Press && key.code == KeyCode::Char('c') {
                if let Some(paper) = papers.get(index) {
                    ok_papers.retain(|p| p.paper.paper_id.is_none() || p.paper.paper_id != paper.paper.paper_id);
                    ok_papers.push(IncludedPaper {
                        paper: paper.paper.clone(),
                        status: IncludedPaperStatus::CoreLiterature,
                        message: None,
                    });
                    excluded_papers.retain(|p| p.paper_id.is_none() || p.paper_id != paper.paper.paper_id);
                    save = true;
                }
            }
            if mode == Mode::Sifting && key.kind == KeyEventKind::Press && key.code == KeyCode::Backspace {
                if let Some(paper) = papers.get(index) {
                    excluded_papers.retain(|p| p.paper_id.is_none() || p.paper_id != paper.paper.paper_id);
                    excluded_papers.push(paper.paper.clone());
                    ok_papers.retain(|p| p.paper.paper_id.is_none() || p.paper.paper_id != paper.paper.paper_id);
                    save = true;
                }
            }
            if mode == Mode::Sifting && key.kind == KeyEventKind::Press && key.code == KeyCode::Delete {
                if let Some(paper) = papers.get(index) {
                    excluded_papers.retain(|p| p.paper_id.is_none() || p.paper_id != paper.paper.paper_id);
                    ok_papers.retain(|p| p.paper.paper_id.is_none() || p.paper.paper_id != paper.paper.paper_id);
                    save = true;
                }
            }

            if mode == Mode::Sifting && key.kind == KeyEventKind::Press && key.code == KeyCode::Char('r') {
                term.clear()?;
            }

            if mode == Mode::Comment && papers.get(index).is_none() {
                mode = Mode::Sifting;
            }

            let mut current_paper = if let Some(paper) = papers.get(index) {
                ok_papers.iter_mut().find(|p| p.paper.paper_id == paper.paper.paper_id)
            } else {
                None
            };
            if mode == Mode::Comment && current_paper.is_none() {
                mode = Mode::Sifting;
            }

            if let Some(paper) = &mut current_paper {
                if mode == Mode::Comment && key.kind == KeyEventKind::Press {
                    if key.code == KeyCode::Enter || key.code == KeyCode::Esc {
                        mode = Mode::Sifting;
                        save = true;
                    }

                    if let KeyCode::Char(c) = key.code {
                        if let Some(msg) = &mut paper.message {
                            msg.push(c);
                        } else {
                            paper.message = Some(c.to_string());
                        }
                    }

                    if key.code == KeyCode::Backspace {
                        if let Some(msg) = &mut paper.message {
                            msg.pop();
                        }
                    }

                    let mut delete = false;
                    if let Some(msg) = &mut paper.message {
                        *msg = msg.trim().to_string();
                        delete = msg.len() == 0;
                    }
                    if delete {
                        paper.message = None;
                    }
                }
            }

            if mode == Mode::Sifting && key.kind == KeyEventKind::Press && key.code == KeyCode::Char('m') {
                if current_paper.is_some() {
                    mode = Mode::Comment;
                }
            }

            if save {
                if excluded_papers.len() < excluded_paper_len.max(1)-1 || ok_papers.len() < ok_paper_len.max(1)-1 {
                    panic!("Error: deleted more than one paper"); // failsafe
                }
                if excluded_papers.len() > excluded_paper_len+1 || ok_papers.len() > ok_paper_len+1 {
                    panic!("Error: added more than one paper"); // failsafe
                }

                let ok_data = serde_json::to_string_pretty(&ok_papers)?;
                std::fs::write("ok_papers.json", ok_data)?;

                let excluded_data = serde_json::to_string_pretty(&excluded_papers)?;
                std::fs::write("excluded_papers.json", excluded_data)?;
            }
        }
    }
}

fn load_sifted_papers(all_papers: &Vec<RelevancePaper>, only_core_literature: bool) -> Result<Vec<RelevancePaper>> {
    let papers_ok = std::fs::read_to_string("ok_papers.json")
        .map(|data| serde_json::from_str::<Vec<IncludedPaper>>(&data))??;

    let mut full_paper_info: Vec<RelevancePaper> = Vec::with_capacity(papers_ok.len());
    for ipaper in &papers_ok {
        if let Some(paper) = all_papers.iter().find(|p| p.paper.paper_id == Some(ipaper.paper.paper_id.clone().unwrap())) {
            if ipaper.status == CoreLiterature || !only_core_literature {
                full_paper_info.push(paper.clone());
            }
        }
    }

    if !only_core_literature {
        assert_eq!(full_paper_info.len(), papers_ok.len(), "paper not found in database");
    }

    Ok(full_paper_info)
}

async fn expand_papers(all_papers: &mut Vec<RelevancePaper>) -> Result<()> {
    async fn expand_paper(all_papers: &mut Vec<RelevancePaper>, related_paper: RelevancePaperWeak) -> Result<()> {
        let mut changes = false;

        let paper_id = related_paper.paper_id.clone();
        if let Some(paper_id) = paper_id {
            if all_papers.iter().any(|p| p.paper.paper_id == Some(paper_id.clone())) {
                println!("SKIP {}", related_paper.title.unwrap_or("".into()));
                return Ok(());
            }

            println!("+ {}", related_paper.title.unwrap_or("".into()));
            let paper = query_paper_data(paper_id).await?;
            all_papers.push(paper);
            changes = true;
        } else {
            println!("NOID {}", related_paper.title.unwrap_or("".into()));
        }

        if changes {
            let data = serde_json::to_string_pretty(&all_papers)?;
            std::fs::write("database_papers.json", data)?;
        }

        Ok(())
    }

    for core_paper in load_sifted_papers(&all_papers, true)? {
        let mut related = core_paper.references.clone();
        related.append(&mut core_paper.citations.clone());

        for related_paper in related {
            expand_paper(all_papers, related_paper).await?;
        }
    }

    Ok(())
}


#[tokio::main]
async fn main() ->  Result<()> {
    let semantic_scholar_key = std::fs::read_to_string("api.key").map(Some).unwrap_or(None);
    *SEMANTIC_SCHOLAR_API_KEY.lock().await.deref_mut() = semantic_scholar_key;

    // keyword_search_all().await?;

    let mut papers = read_paper_database()?;
    println!("Total papers in database: {}", papers.len());

    loop {
        let mut terminal = ratatui::init();
        let result = sifting(&papers, &mut terminal)?;
        drop(terminal);
        ratatui::restore();

        if !result {
            break;
        }

        expand_papers(&mut papers).await?;
    }


    let total = load_sifted_papers(&papers, false)?;
    println!("Filtered count: {}", total.len());

    Ok(())
}