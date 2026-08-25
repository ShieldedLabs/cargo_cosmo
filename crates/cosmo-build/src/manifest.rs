//! Read what to build out of `[package.metadata.cosmo]`.
//!
//! An APE is a program, and a library crate has none, so something must name
//! one. The build script cannot learn it from the command line -- cargo hands
//! an identical environment to `cargo build` and `cargo build --example demo`
//! -- so it is declared instead, next to the crate's other target declarations:
//!
//! ```toml
//! [package.metadata.cosmo]
//! example  = "demo"              # or ["demo", "bench"]
//! bin      = "tool"              # or [...]
//! features = ["gui", "x11"]
//! args     = ["--no-default-features"]
//! ```
//!
//! Cargo ignores everything under `package.metadata`, so this costs a native
//! build nothing.

use std::path::Path;

/// Turn the crate's `[package.metadata.cosmo]` table into cargo arguments.
///
/// An absent table is not an error: a crate with bins wants none of this.
pub fn args(manifest_dir: &Path) -> Result<Vec<String>, String> {
   let path = manifest_dir.join("Cargo.toml");
   let text = std::fs::read_to_string(&path)
      .map_err(|e| format!("{}: {e}", path.display()))?;
   Ok(parse(&text))
}

/// A scan rather than a TOML parse.
///
/// The shape being read is one flat table of strings and string arrays, and a
/// TOML crate in the dependency tree of every consuming build is a poor trade
/// for that. The cost is that only the standard header form is understood --
/// `[package.metadata.cosmo]` with plain `key = value` lines under it, not the
/// inline-table spelling `metadata = { cosmo = { ... } }`, which is why the
/// docs show the header form.
fn parse(toml: &str) -> Vec<String> {
   let mut out = Vec::new();
   let mut inside = false;
   let mut pending: Option<String> = None; // an array still being collected

   for raw in toml.lines() {
      let line = strip_comment(raw).trim();
      if line.is_empty() {
         continue;
      }

      if let Some(open) = pending.take() {
         let done = line.contains(']');
         out.extend(flag_values(&open, line));
         if !done {
            pending = Some(open);
         }
         continue;
      }

      if line.starts_with('[') {
         inside = line == "[package.metadata.cosmo]";
         continue;
      }
      if !inside {
         continue;
      }

      let Some((key, value)) = line.split_once('=') else {
         continue;
      };
      let (key, value) = (key.trim(), value.trim());
      let flag = match key {
         "example" => "--example",
         "bin" => "--bin",
         "features" => "--features",
         "args" => "",
         _ => continue,
      };

      // An array may run to the end of the line or over several of them.
      if value.starts_with('[') && !value.contains(']') {
         out.extend(flag_values(flag, value));
         pending = Some(flag.to_string());
         continue;
      }
      out.extend(flag_values(flag, value));
   }
   out
}

/// Emit `flag value` for every quoted string in `value`. An empty flag means
/// the strings are already whole arguments.
fn flag_values(flag: &str, value: &str) -> Vec<String> {
   let mut out = Vec::new();
   for s in quoted(value) {
      if !flag.is_empty() {
         out.push(flag.to_string());
      }
      out.push(s);
   }
   out
}

fn quoted(s: &str) -> Vec<String> {
   let mut out = Vec::new();
   let mut chars = s.chars();
   while let Some(c) = chars.next() {
      if c != '"' {
         continue;
      }
      let mut val = String::new();
      for c in chars.by_ref() {
         if c == '"' {
            break;
         }
         val.push(c);
      }
      out.push(val);
   }
   out
}

/// Drop a trailing `#` comment, but not a `#` inside a string.
fn strip_comment(line: &str) -> &str {
   let mut in_str = false;
   for (i, c) in line.char_indices() {
      match c {
         '"' => in_str = !in_str,
         '#' if !in_str => return &line[..i],
         _ => {}
      }
   }
   line
}

#[cfg(test)]
mod tests {
   use super::parse;

   #[test]
   fn single_example() {
      let t = "[package]\nname = \"x\"\n\n[package.metadata.cosmo]\nexample = \"demo\"\n";
      assert_eq!(parse(t), ["--example", "demo"]);
   }

   #[test]
   fn arrays_and_features() {
      let t = "[package.metadata.cosmo]\nexample = [\"a\", \"b\"]\nfeatures = [\"gui\"]\n";
      assert_eq!(parse(t), ["--example", "a", "--example", "b", "--features", "gui"]);
   }

   #[test]
   fn raw_args_pass_through_unflagged() {
      let t = "[package.metadata.cosmo]\nargs = [\"--no-default-features\"]\n";
      assert_eq!(parse(t), ["--no-default-features"]);
   }

   /// Keys outside the table must not leak in -- `example` is a plausible key
   /// name for somebody else's metadata table.
   #[test]
   fn other_tables_are_ignored() {
      let t = "[package.metadata.other]\nexample = \"nope\"\n\n[package.metadata.cosmo]\nbin = \"yes\"\n";
      assert_eq!(parse(t), ["--bin", "yes"]);
   }

   #[test]
   fn absent_table_is_empty() {
      assert_eq!(parse("[package]\nname = \"x\"\n"), Vec::<String>::new());
   }

   #[test]
   fn comments_and_spacing() {
      let t = "[package.metadata.cosmo]  # what the APE contains\n  example  =  \"demo\"  # the gui\n";
      assert_eq!(parse(t), ["--example", "demo"]);
   }

   #[test]
   fn a_hash_inside_a_string_is_not_a_comment() {
      let t = "[package.metadata.cosmo]\nexample = \"de#mo\"\n";
      assert_eq!(parse(t), ["--example", "de#mo"]);
   }

   #[test]
   fn multiline_array() {
      let t = "[package.metadata.cosmo]\nexample = [\n  \"a\",\n  \"b\",\n]\nfeatures = [\"g\"]\n";
      assert_eq!(parse(t), ["--example", "a", "--example", "b", "--features", "g"]);
   }
}
