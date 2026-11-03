//! The state tool's executable: one call handed by the tool host on standard input, answered on standard
//! output (bmo-pocket's `docs/mcp-host.md`, 「A call」).

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
  state_tool::run().await
}
