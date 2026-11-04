//! Words meant for the owner that hand him a page hosted outside the system's own archive (docs/cli.md): such a
//! page is out of his reach once the session that made it is gone, and on somebody else's terms meanwhile.

use url::Url;

/// Whether an address opens a page published by the tool that made it, rather than one read from as a source.
fn hosted_outside(address: &Url) -> bool {
  let Some(host) = address.host_str() else { return false };
  let host = host.to_ascii_lowercase();
  let is = |name: &str| host == name || host.ends_with(&format!(".{name}"));

  (is("claude.ai") && address.path().split('/').any(|part| part == "artifact" || part == "artifacts"))
    || is("claude.site")
    || is("claudeusercontent.com")
    || is("gist.github.com")
    || is("gist.githubusercontent.com")
    || is("pastebin.com")
}

/// Every address in the texts that opens a page hosted outside, in the order they stand, each once.
pub fn outside_addresses(texts: &[&str]) -> Vec<String> {
  let mut found: Vec<String> = vec![];

  for text in texts {
    for word in text.split(|at: char| at.is_whitespace() || "()<>[]'\"`".contains(at)) {
      let word = word.trim_end_matches(['.', ',', ';', ':', '!', '?']);

      if (word.starts_with("http://") || word.starts_with("https://"))
        && Url::parse(word).is_ok_and(|address| hosted_outside(&address))
        && !found.iter().any(|kept| kept == word)
      {
        found.push(word.to_owned());
      }
    }
  }

  found
}

/// The refusal of words holding such an address, or none where they hold none.
pub fn outside_refusal(texts: &[&str]) -> Option<String> {
  let found = outside_addresses(texts);

  (!found.is_empty()).then(|| {
    format!(
      "a document shown to the owner goes into the system's own archive (bmo archive doc), never onto a page hosted \
       outside it: {}",
      found.join(", ")
    )
  })
}
