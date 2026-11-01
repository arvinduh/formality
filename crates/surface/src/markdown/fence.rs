//! Formats the code in Markdown's fenced blocks with the other languages.
//!
//! Owns which info strings name a language and how a batch of blocks reaches
//! that language's formatter: each block becomes a file in a hidden temp dir
//! under the workspace root, so the tools find the workspace's configs, and
//! each language formats all of its blocks in one run. dprint owns where the
//! blocks are and how they are written back.

use std::collections;
use std::fs;
use std::io;

use tempfile;

/// Block code by the extension of the language it is in. Each block maps to
/// its formatted text, or to itself until it is formatted.
pub type Blocks =
  collections::HashMap<&'static str, collections::HashMap<String, String>>;

/// The extension of the language that `tag`, a fence's info string up to the
/// first space or comma, names, such as `rs` for `rust`. Markdown names
/// none, since dprint formats nested Markdown itself.
pub fn extension(tag: &str) -> Option<&'static str> {
  let tag = tag.to_ascii_lowercase();
  let tag = match tag.as_str() {
    "rust" => "rs",
    "python" => "py",
    "javascript" => "js",
    "typescript" => "ts",
    "shell" => "sh",
    "c++" => "cpp",
    other => other,
  };
  crate::SURFACES
    .iter()
    .filter(|surface| surface.name() != "markdown")
    .flat_map(|surface| surface.extensions())
    .find(|extension| **extension == tag)
    .copied()
}

/// Replaces each block in `blocks` with its language's formatting of it.
///
/// A block stays as written when its language's tool is missing or fails
/// on it, such as on a fragment that does not parse.
///
/// # Errors
///
/// Returns an error when the temp dir or its files cannot be written.
pub fn format(
  ctx: &crate::Context,
  blocks: &mut Blocks,
) -> Result<(), io::Error> {
  let dir = tempfile::Builder::new()
    .prefix(".fml-")
    .tempdir_in(ctx.root)?;
  let mut files = Vec::new();
  for (extension, codes) in blocks.iter() {
    for code in codes.keys() {
      let path = dir.path().join(format!("{}.{extension}", files.len()));
      fs::write(&path, format!("{code}\n"))?;
      files.push((path, *extension, code.clone()));
    }
  }
  for surface in crate::SURFACES {
    let batch: Vec<_> = files
      .iter()
      .filter(|(_, extension, _)| surface.extensions().contains(extension))
      .map(|(path, _, _)| path.clone())
      .collect();
    if batch.is_empty() {
      continue;
    }
    // One block that does not parse fails the whole batch, so retry each
    // block alone to format the rest.
    if surface.format(ctx, &batch, false).is_err() {
      for file in &batch {
        drop(surface.format(ctx, std::slice::from_ref(file), false));
      }
    }
  }
  for (path, extension, code) in files {
    if let (Ok(tidy), Some(codes)) =
      (fs::read_to_string(&path), blocks.get_mut(extension))
    {
      codes.insert(code, tidy);
    }
  }
  Ok(())
}
