use serde::de::DeserializeOwned;
use serde_json::json;
use std::{env, io::Read, process::ExitCode};
use zkr::{
    CorrectInput, DeleteInput, EmbeddingInput, MemoryDb, ProjectionAuditInput, ReviewInput,
    ReviewsInput, SearchInput,
};

const HELP: &str = "zkr --db PATH COMMAND\n\nCommands (read one JSON object from stdin; write JSON to stdout):\n  remember     Store source evidence and an optional claim\n  search       Retrieve bounded, cited memory matches\n  correct      Supersede a claim using new correction evidence\n  delete       Tombstone a source and propagate unavailable evidence\n  review       Store a cited daily review without invoking an LLM\n  reviews      Retrieve bounded daily reviews\n  projections  List bounded stale or missing embedding inputs\n  embed        Upsert a rebuildable embedding projection\n  help         Show this help\n";

fn main() -> ExitCode {
    match run() {
        Ok(value) => {
            if let Some(value) = value {
                println!("{value}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", json!({ "error": error.to_string() }));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<Option<serde_json::Value>, Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.as_slice() == ["help"]
        || arguments.as_slice() == ["--help"]
        || arguments.as_slice() == ["-h"]
        || arguments.is_empty()
    {
        print!("{HELP}");
        return Ok(None);
    }
    if arguments.len() != 3 || arguments[0] != "--db" {
        return Err("usage: zkr --db PATH COMMAND (use --help)".into());
    }
    let mut database = MemoryDb::open(&arguments[1])?;
    let value = match arguments[2].as_str() {
        "remember" => serde_json::to_value(database.remember(read_json()?)?)?,
        "search" => serde_json::to_value(database.search(read_json::<SearchInput>()?)?)?,
        "correct" => serde_json::to_value(database.correct(read_json::<CorrectInput>()?)?)?,
        "delete" => serde_json::to_value(database.delete_source(read_json::<DeleteInput>()?)?)?,
        "review" => serde_json::to_value(database.store_review(read_json::<ReviewInput>()?)?)?,
        "reviews" => serde_json::to_value(database.reviews(read_json::<ReviewsInput>()?)?)?,
        "projections" => {
            serde_json::to_value(database.projection_issues(read_json::<ProjectionAuditInput>()?)?)?
        }
        "embed" => {
            serde_json::to_value(database.upsert_embedding(read_json::<EmbeddingInput>()?)?)?
        }
        command => return Err(format!("unknown command {command:?}").into()),
    };
    Ok(Some(value))
}

fn read_json<T: DeserializeOwned>() -> Result<T, Box<dyn std::error::Error>> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    Ok(serde_json::from_str(&input)?)
}
