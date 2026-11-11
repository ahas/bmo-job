//! The chat agent's launching entries for the tool host (the chat agent's `bmo` namespace): a job filed on a
//! worker, which reports to the room when it ends, and a research sent out at a depth, whose result comes back
//! to the room. The job is filed through the agency's `POST /item-filings`, so the turn's room and whether the
//! owner asked it are the agency's reading rather than anything the model writes; the research is asked of the
//! `research/run` entry the session holds, on the call's own socket back into the host.

use serde::Deserialize;
use serde_json::{Value, json};

use tool_call::{
  Answer, Handed, call_back, entry_name, queue_name, read_handed, result_failed, result_structured,
  result_text, shortened, turn_agent, turn_room, write_answer,
};

/// The entries this executable answers, as its manifest declares them.
pub const ENTRIES: [&str; 2] = ["task", "search"];

/// A job filed on a worker.
#[derive(Debug, Deserialize)]
pub struct TaskArgs {
  pub prompt: String,
  #[serde(default)]
  pub chat: Option<i64>,
  #[serde(default)]
  pub worker: Option<String>,
  #[serde(default)]
  pub model: Option<String>,
  #[serde(default)]
  pub todo: Option<i64>,
  #[serde(default)]
  pub why: Option<String>,
}

/// A research sent out.
#[derive(Debug, Default, Deserialize)]
pub struct SearchArgs {
  #[serde(default)]
  pub query: Option<String>,
  #[serde(default)]
  pub import: Option<String>,
  #[serde(default)]
  pub depth: Option<String>,
  #[serde(default)]
  pub demand: Option<String>,
  #[serde(default)]
  pub period: Option<String>,
  #[serde(default)]
  pub region: Option<String>,
  #[serde(default)]
  pub shape: Option<String>,
  #[serde(default)]
  pub exclude: Option<String>,
  #[serde(default)]
  pub chat: Option<i64>,
}

/// The research depth a search's `depth` names, `search:basic` where it names none.
pub fn key_of(depth: Option<&str>) -> Result<&'static str, String> {
  match depth.unwrap_or("basic") {
    "basic" => Ok("search:basic"),
    "thorough" => Ok("search:thorough"),
    "exhaustive" => Ok("search:exhaustive"),
    other => Err(format!("depth is {other:?}; it is basic, thorough or exhaustive")),
  }
}

/// The first line of a prompt, as a job's title.
pub fn title_of(prompt: &str) -> String {
  let first = prompt.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("A job");

  shortened(first, 80)
}

/// The question a search asks: its words, or the page an import brings in.
pub fn question_of(args: &SearchArgs) -> Result<String, String> {
  match (args.query.as_deref().map(str::trim), args.import.as_deref().map(str::trim)) {
    (Some(query), _) if !query.is_empty() => Ok(query.to_owned()),
    (_, Some(import)) if !import.is_empty() => Ok(format!("Bring the page at {import} in whole.")),
    _ => Err("nothing was asked: give query, the question, or import, the address to bring in".to_owned()),
  }
}

/// The options a search hands the research, from the shape, span, region and sources it names.
pub fn options_of(args: &SearchArgs) -> Option<String> {
  let parts: Vec<String> = [
    ("shape", args.shape.as_deref()),
    ("period", args.period.as_deref()),
    ("region", args.region.as_deref()),
    ("exclude", args.exclude.as_deref()),
    ("demand", args.demand.as_deref()),
  ]
  .into_iter()
  .filter_map(|(name, value)| {
    value
      .map(str::trim)
      .filter(|value| !value.is_empty())
      .map(|value| format!("{name}: {value}"))
  })
  .collect();

  if parts.is_empty() { None } else { Some(parts.join("; ")) }
}

/// The entries: a job filed and a research sent out.
pub async fn answer(handed: &Handed) -> Answer {
  let name = entry_name(&handed.entry);
  if !ENTRIES.contains(&name) {
    return Answer::refusal(format!("this executable holds no entry {}", handed.entry));
  }

  match name {
    "task" => task(handed).await,
    "search" => search(handed).await,
    other => Answer::refusal(format!("this executable holds no entry {other}")),
  }
}

async fn task(handed: &Handed) -> Answer {
  let args: TaskArgs = match serde_json::from_value(handed.arguments.clone()) {
    Ok(args) => args,
    Err(why) => return Answer::refusal(format!("task: {why}")),
  };
  if let Some(item) = args.todo {
    return Answer::refusal(format!(
      "item #{item} is already filed: a worker leases it from its queue when it comes due, so no new item is filed for it"
    ));
  }
  let Some(agent) = turn_agent(&handed.session) else {
    return Answer::refusal("this call names no agent to file for");
  };
  let service = match tool_call::agency_service(&handed.session) {
    Ok(service) => service,
    Err(why) => return Answer::refusal(why),
  };
  let mut labels = serde_json::Map::new();
  if let Some(model) = args.model.as_deref().filter(|model| !model.is_empty()) {
    labels.insert("model".to_owned(), json!(model));
  }
  if let Some(why) = args.why.as_deref().filter(|why| !why.is_empty()) {
    labels.insert("why".to_owned(), json!(why));
  }
  let body = json!({
    "agentName": agent,
    "title": title_of(&args.prompt),
    "brief": args.prompt,
    "queueName": queue_name(&handed.session),
    "repository": args.worker.filter(|worker| !worker.is_empty()),
    "labels": labels,
  });

  match tool_call::post(&service, "/item-filings", &body).await {
    Ok(filed) => {
      let id = filed.get("id").and_then(Value::as_i64).unwrap_or(0);
      let state = filed.get("state").and_then(Value::as_str).unwrap_or_default();
      let scope = filed.get("scope").cloned().unwrap_or(Value::Null);
      Answer::structured(json!({ "itemId": id, "state": state, "scope": scope }))
    }
    Err(why) => Answer::refusal(why),
  }
}

async fn search(handed: &Handed) -> Answer {
  let args: SearchArgs = match serde_json::from_value(handed.arguments.clone()) {
    Ok(args) => args,
    Err(why) => return Answer::refusal(format!("search: {why}")),
  };
  let key = match key_of(args.depth.as_deref()) {
    Ok(key) => key,
    Err(why) => return Answer::refusal(why),
  };
  let query = match question_of(&args) {
    Ok(query) => query,
    Err(why) => return Answer::refusal(why),
  };
  let Some(room) = args.chat.or_else(|| turn_room(&handed.session)) else {
    return Answer::refusal(
      "this call names no room for the result: give chat, the room's id, or make it in a turn that answers one",
    );
  };
  let Some(socket) = handed.callback.as_deref() else {
    return Answer::refusal("this call was handed no call-back socket, so the research cannot be asked");
  };
  let mut ask = json!({ "key": key, "query": query, "roomId": room });
  if let Some(asker) = turn_agent(&handed.session) {
    ask["asker"] = json!(asker);
  }
  if let Some(options) = options_of(&args) {
    ask["options"] = json!(options);
  }

  match call_back(socket, "research/run", ask).await {
    Ok(result) if result_failed(&result) => Answer::refusal(result_text(&result)),
    Ok(result) => match result_structured(&result) {
      Some(value) => Answer::structured(value),
      None => Answer::text(result_text(&result)),
    },
    Err(why) => Answer::refusal(why),
  }
}

/// The executable's whole main.
pub async fn run() -> std::process::ExitCode {
  let handed = match read_handed() {
    Ok(handed) => handed,
    Err(why) => {
      eprintln!("{why}");
      return std::process::ExitCode::FAILURE;
    }
  };

  if handed.protocol != tool_call::PROTOCOL {
    return write_answer(&Answer::refusal(format!(
      "this tool speaks protocol {}, and was handed {}",
      tool_call::PROTOCOL,
      handed.protocol
    )));
  }

  if handed.kind != "call" {
    return write_answer(&Answer::refusal(format!("this tool answers calls alone, not {}", handed.kind)));
  }

  write_answer(&answer(&handed).await)
}
