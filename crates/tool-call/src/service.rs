//! The service as a tool reaches it (docs/tools.md, "The tools and their entries"): its API at the
//! `[server] address` of the settings file its own verbs read, and its unit CLI, `bmo-job` on the search path.

use std::time::Duration;

use job_base::Settings;
use serde_json::Value;
use url::Url;

/// The unit CLI's program, as the platform's naming rule finds it (docs/cli.md).
pub const CLI_PROGRAM: &str = "bmo-job";

// A call is the host's to time out; a request waits no longer than the longest call takes.
const WAIT: Duration = Duration::from_secs(170);

/// Where a tool reaches the service.
#[derive(Clone, Debug)]
pub struct Service {
  pub server: Url,
}

impl Service {
  /// The service the settings file of this process names, or why it names none.
  pub fn of_process() -> Result<Self, String> {
    let settings = Settings::load(&Settings::place()).map_err(|why| why.to_string())?;
    let server = settings
      .server
      .as_ref()
      .ok_or("the settings have no [server] section, which a tool reaches the service by")?;

    Ok(Self {
      server: server.address.url().clone(),
    })
  }

  fn url(&self, path: &str) -> Result<Url, String> {
    self.server.join(path).map_err(|why| format!("{path}: {why}"))
  }

  /// A reading of the route at these path segments, each encoded as a segment, with its query: what it
  /// answered, or the sentence it was refused with.
  pub async fn get(&self, segments: &[&str], query: &[(&str, String)]) -> Result<Value, String> {
    let mut url = self.server.clone();

    url
      .path_segments_mut()
      .map_err(|()| format!("{} takes no path", self.server))?
      .pop_if_empty()
      .extend(segments);

    if !query.is_empty() {
      url.query_pairs_mut().extend_pairs(query);
    }
    answered(reqwest::Client::new().get(url).timeout(WAIT)).await
  }

  /// A request of one route: what it answered, or the sentence it was refused with.
  pub async fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
    answered(reqwest::Client::new().post(self.url(path)?).json(body).timeout(WAIT)).await
  }

  /// The unit CLI run on `words` with the turn's `variables`: the lines it printed, or the sentence it ended
  /// on where it did not end with success.
  pub async fn cli(&self, words: &[String], variables: &[(&str, String)]) -> Result<Vec<String>, String> {
    let ran = tokio::process::Command::new(CLI_PROGRAM)
      .args(words)
      .envs(variables.iter().map(|(name, value)| (*name, value.as_str())))
      .stdin(std::process::Stdio::null())
      .output()
      .await
      .map_err(|why| format!("{CLI_PROGRAM} could not be started: {why}"))?;
    let lines: Vec<String> = String::from_utf8_lossy(&ran.stdout).lines().map(str::to_owned).collect();

    if ran.status.success() {
      return Ok(lines);
    }

    let said = String::from_utf8_lossy(&ran.stderr).trim().to_owned();

    Err(if said.is_empty() {
      format!(
        "{CLI_PROGRAM} ended with {}",
        ran.status.code().map_or_else(|| "a signal".to_owned(), |code| code.to_string())
      )
    } else {
      said
    })
  }
}

async fn answered(request: reqwest::RequestBuilder) -> Result<Value, String> {
  let answer = request.send().await.map_err(|why| format!("the service did not answer: {why}"))?;
  let status = answer.status();
  let read: Value = answer.json().await.unwrap_or(Value::Null);

  if status.is_success() {
    return Ok(read);
  }

  Err(
    read
      .get("error")
      .and_then(Value::as_str)
      .map_or_else(|| format!("the service answered {status}"), str::to_owned),
  )
}
