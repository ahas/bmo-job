//! The webhook's signature: an HMAC-SHA256 of the body under the subscription's secret (docs/api.md,
//! "Subscriptions").

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// The header carrying the subscription's key.
pub const SUBSCRIPTION_HEADER: &str = "Job-Subscription";

/// The header carrying the body's signature, where the subscription has a secret.
pub const SIGNATURE_HEADER: &str = "Job-Signature";

/// The signature of `body` under `secret`, as the header carries it: `sha256=` and the lowercase hex digest.
pub fn signature(secret: &str, body: &[u8]) -> String {
  let mut mac =
    Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("an HMAC takes a key of any length");
  mac.update(body);
  format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

/// Whether `header` is the signature of `body` under `secret`, compared in constant time.
pub fn signed_by(secret: &str, body: &[u8], header: &str) -> bool {
  let Some(digest) = header.strip_prefix("sha256=").and_then(|hex| hex::decode(hex).ok()) else {
    return false;
  };
  let mut mac =
    Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("an HMAC takes a key of any length");
  mac.update(body);
  mac.verify_slice(&digest).is_ok()
}
