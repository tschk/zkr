use serde::de::DeserializeOwned;
use serde_json::json;
use std::{env, io::Read, process::ExitCode};
use zkr::{
    ArchiveInput, ClaimEvidence, CorrectInput, DeleteInput, EmbeddingInput, EvidenceLocatorInput,
    ExportInput, GetInput, MemoryDb, ProfileInput, ProfilesInput, ProjectionAuditInput,
    PromoteInput, RememberRequest, RepairInput, ReviewInput, ReviewsInput, SearchInput,
};

const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

const HELP: &str = "zkr --db PATH COMMAND\n\nCommands (read one JSON object from stdin; write JSON to stdout):\n  remember     Store source evidence and an optional typed claim or transcript locator\n  locator      Read a live evidence transcript locator\n  search       Retrieve bounded, cited memory matches\n  get          Read one live cited memory by target\n  correct      Supersede a claim using new correction evidence\n  promote      Promote a short-term processed claim to long-term\n  archive      Move a processed claim to archive tier\n  link         Attach supporting or contradicting evidence to a claim\n  delete       Tombstone a source and propagate unavailable evidence\n  repair       Process bounded projection-repair outbox records\n  profile      Project a live profile-fact claim into the current profile\n  profiles     Retrieve bounded live profile entries\n  review       Store a cited daily review without invoking an LLM\n  reviews      Retrieve bounded daily reviews\n  projections  List bounded stale or missing embedding inputs\n  embed        Upsert a rebuildable embedding projection\n  export       Read a bounded page from the scoped authoritative commit feed\n  help         Show this help\n";

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
    if arguments.len() == 3 && arguments[0] == "--db" && arguments[2] == "help" {
        print!("{HELP}");
        return Ok(None);
    }
    if arguments.len() != 3 || arguments[0] != "--db" {
        return Err("usage: zkr --db PATH COMMAND (use --help)".into());
    }
    let mut database = MemoryDb::open(&arguments[1])?;
    let value = match arguments[2].as_str() {
        "remember" => {
            let request = read_json::<RememberRequest>()?;
            serde_json::to_value(database.remember_with_locator(request.memory, request.locator)?)?
        }
        "locator" => {
            serde_json::to_value(database.evidence_locator(read_json::<EvidenceLocatorInput>()?)?)?
        }
        "search" => serde_json::to_value(database.search(read_json::<SearchInput>()?)?)?,
        "get" => serde_json::to_value(database.get(read_json::<GetInput>()?)?)?,
        "correct" => serde_json::to_value(database.correct(read_json::<CorrectInput>()?)?)?,
        "link" => {
            database.link_claim_evidence(read_json::<ClaimEvidence>()?)?;
            serde_json::json!({"ok": true})
        }
        "promote" => serde_json::to_value(database.promote(read_json::<PromoteInput>()?)?)?,
        "archive" => serde_json::to_value(database.archive(read_json::<ArchiveInput>()?)?)?,
        "delete" => serde_json::to_value(database.delete_source(read_json::<DeleteInput>()?)?)?,
        "repair" => {
            serde_json::to_value(database.repair_projections(read_json::<RepairInput>()?)?)?
        }
        "profile" => serde_json::to_value(database.store_profile(read_json::<ProfileInput>()?)?)?,
        "profiles" => serde_json::to_value(database.profiles(read_json::<ProfilesInput>()?)?)?,
        "review" => serde_json::to_value(database.store_review(read_json::<ReviewInput>()?)?)?,
        "reviews" => serde_json::to_value(database.reviews(read_json::<ReviewsInput>()?)?)?,
        "projections" => {
            serde_json::to_value(database.projection_issues(read_json::<ProjectionAuditInput>()?)?)?
        }
        "embed" => {
            serde_json::to_value(database.upsert_embedding(read_json::<EmbeddingInput>()?)?)?
        }
        "export" => serde_json::to_value(database.export(read_json::<ExportInput>()?)?)?,
        command => return Err(format!("unknown command {command:?}").into()),
    };
    Ok(Some(value))
}

fn read_json<T: DeserializeOwned>() -> Result<T, Box<dyn std::error::Error>> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut input)?;
    if input.len() as u64 > MAX_REQUEST_BYTES {
        return Err("request exceeds 1048576 bytes".into());
    }
    Ok(serde_json::from_slice(&input)?)
}
