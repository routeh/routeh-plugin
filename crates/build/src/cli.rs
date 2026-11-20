//! The commands, the same in a plugin's `cargo xtask` and in the
//! `routeh-plugin` program.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::cmd::{fetch, read, stdout};
use crate::project::Project;
use crate::{bundle, docker, feed, make, openwrt, qemu};

const USAGE: &str = "\
cargo xtask <command>                         (in a plugin's repository)
routeh-plugin <command>                       (the same, as a program)

A plugin:
  build     [--openwrt R] [--targets T]       everything: the service, the modules, bundles, checked
  qemu-test [--openwrt R]                     the modules and [qemu] tests on OpenWrt in QEMU
  id                                          the plugin's id
  version                                     the plugin's version
  targets   [--openwrt R] [--targets T]       targets of a release, as JSON (a CI matrix)
  service   --rust TRIPLE                     the service for a Rust target
  kmod      --openwrt R --target T            the modules with the release's SDK
  bundle    --openwrt R --target T            the bundle of a target, unsigned (kmod and service first)
  missing   --index URL|FILE [--openwrt R]    targets the feed has no build of this version for

Bundles and keys:
  verify    FILE... [--trust PUBLIC]          check bundles as the router reads them
  sign      FILE --key SECRET [--out FILE]    a bundle signed (a developer's own key)
  keygen    SECRET PUBLIC [COMMENT]           a key pair in the router's form

The feed (docs/RELEASE.md):
  feed      --feed DIR --key SECRET [--incoming DIR] [--policy FILE] [--refresh] [--expires-days 90]
            the feed's run: proposed bundles signed in, its index rebuilt and signed
  publish   --feed DIR --key SECRET [--expires-days 90] [BUNDLE...]
            a plugin's own feed (S3): bundles signed into DIR/blobs, added to
            DIR/index.json, signed; no bundles: the index signed again

  R  a release (25.12.6) or `stable` (the default)
  T  `all` (the default: build.toml) or a comma list (mediatek/filogic,armsr/armv8)
  --native  the SDK and cargo on this host, not in Docker (x86_64 Linux: CI)
";

struct Args {
    command: Option<String>,
    flags: Vec<(String, Option<String>)>,
    rest: Vec<String>,
}

/// Flags that take no value.
const SWITCHES: [&str; 2] = ["native", "refresh"];

impl Args {
    fn parse() -> Self {
        let mut it = std::env::args().skip(1).peekable();
        let command = it.next();
        let (mut flags, mut rest) = (Vec::new(), Vec::new());
        while let Some(a) = it.next() {
            if let Some(name) = a.strip_prefix("--") {
                let value = match it.peek() {
                    Some(v) if !v.starts_with("--") && !SWITCHES.contains(&name) => it.next(),
                    _ => None,
                };
                flags.push((name.to_owned(), value));
            } else {
                rest.push(a);
            }
        }
        Self {
            command,
            flags,
            rest,
        }
    }
    fn get(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| v.as_deref())
    }
    fn need(&self, name: &str) -> Result<&str, String> {
        self.get(name).ok_or_else(|| format!("--{name} is needed"))
    }
    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, _)| n == name)
    }
    fn release(&self) -> Result<String, String> {
        openwrt::release(self.get("openwrt").unwrap_or("stable"))
    }
}

/// The repository the command runs in: cargo's workspace.
fn project() -> Result<Project, String> {
    let manifest = stdout(Command::new("cargo").args([
        "locate-project",
        "--workspace",
        "--message-format",
        "plain",
    ]))?;
    let root = Path::new(manifest.trim())
        .parent()
        .ok_or("no workspace")?
        .to_owned();
    Project::open(&root)
}

/// Runs the command of the program's arguments.
pub fn main() -> ExitCode {
    let args = Args::parse();
    match command(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn command(args: &Args) -> Result<ExitCode, String> {
    let native = args.has("native");
    match args.command.as_deref() {
        Some("build") => {
            let p = project()?;
            build(
                &p,
                &args.release()?,
                args.get("targets").unwrap_or("all"),
                native,
            )?;
        }
        Some("qemu-test") => qemu::test(&project()?, &args.release()?, native)?,
        Some("id") => println!("{}", project()?.id),
        Some("version") => println!("{}", project()?.version),
        Some("targets") => {
            let p = project()?;
            let list =
                openwrt::resolve(&p, &args.release()?, args.get("targets").unwrap_or("all"))?;
            println!("{}", serde_json::to_string(&list).expect("serializable"));
        }
        Some("service") => {
            let p = project()?;
            println!(
                "{}",
                docker::build_service(&p, args.need("rust")?, native)?.display()
            );
        }
        Some("kmod") => {
            let p = project()?;
            docker::build_modules(&p, &args.release()?, args.need("target")?, native)?;
        }
        Some("bundle") => {
            let p = project()?;
            let t = openwrt::resolve(&p, &args.release()?, args.need("target")?)?.remove(0);
            println!("{}", make::bundle(&p, &t)?.display());
        }
        Some("missing") => {
            let p = project()?;
            let source = args.need("index")?;
            let text = if source.starts_with("https://") || source.starts_with("http://") {
                // A feed with no index yet has nothing.
                fetch(source).ok().map(String::into_bytes)
            } else {
                std::fs::read(source).ok()
            };
            let index = text
                .map(|t| routeh_plugin_bundle::feed::Index::parse(&t))
                .transpose()
                .map_err(|e| e.to_string())?;
            let targets = openwrt::resolve(&p, &args.release()?, "all")?;
            let wanted: Vec<(String, Option<String>)> = targets
                .iter()
                .map(|t| {
                    (
                        t.arch.clone(),
                        (!p.modules.is_empty()).then(|| t.kernel.clone()),
                    )
                })
                .collect();
            let lacking = feed::missing(index.as_ref(), &p.id, &p.version, &wanted);
            let names: Vec<&str> = targets
                .iter()
                .zip(&wanted)
                .filter(|(_, w)| lacking.contains(w))
                .map(|(t, _)| t.target.as_str())
                .collect();
            println!("{}", names.join(","));
        }
        Some("verify") if !args.rest.is_empty() => {
            let trust = args.get("trust").map(bundle::read_public).transpose()?;
            for f in &args.rest {
                let info = bundle::check(&read(Path::new(f))?).map_err(|e| format!("{f}: {e}"))?;
                if let Some(t) = trust
                    && info.signer != Some(t)
                {
                    return Err(format!("{f}: not signed by {}", bundle::key_text(&t)));
                }
                println!("{f}: ok, {info}");
            }
        }
        Some("sign") if args.rest.len() == 1 => {
            let key = bundle::read_secret(args.need("key")?)?;
            let from = PathBuf::from(&args.rest[0]);
            let signed =
                routeh_plugin_bundle::sign(&read(&from)?, &key).map_err(|e| e.to_string())?;
            let to = args.get("out").map_or(from, PathBuf::from);
            std::fs::write(&to, signed).map_err(|e| format!("{}: {e}", to.display()))?;
            println!("{}", to.display());
        }
        Some("keygen") if (2..=3).contains(&args.rest.len()) => {
            let k = routeh_plugin_bundle::SecretKey::generate().map_err(|e| e.to_string())?;
            let comment = args.rest.get(2).map_or("", String::as_str);
            create(&args.rest[0], &k.to_line(), 0o600)?;
            create(&args.rest[1], &k.public().to_line(comment), 0o644)?;
        }
        Some("feed") => {
            let dir = PathBuf::from(args.need("feed")?);
            let key = bundle::read_secret(args.need("key")?)?;
            let policy = match args.get("policy") {
                Some(p) => feed::Policy::read(Path::new(p))?,
                None => feed::Policy::read(&dir.join("feed.toml"))?,
            };
            let days = expires_days(args)?;
            let now = now();
            let incoming = args.get("incoming").map(Path::new);
            let r = feed::run(
                &dir,
                &key,
                incoming,
                &policy,
                args.has("refresh"),
                days,
                now,
            )?;
            for a in &r.added {
                println!("added {a}");
            }
            for e in &r.refused {
                println!("refused {e}");
            }
            if r.written {
                println!("index.json: serial {}, expires in {days} days", r.serial);
            } else {
                println!("index.json: unchanged, serial {}", r.serial);
            }
            if !r.refused.is_empty() {
                return Ok(ExitCode::FAILURE);
            }
        }
        Some("publish") => {
            let dir = PathBuf::from(args.need("feed")?);
            let key = bundle::read_secret(args.need("key")?)?;
            let days = expires_days(args)?;
            let bundles: Vec<PathBuf> = args.rest.iter().map(PathBuf::from).collect();
            let r = feed::publish(&dir, &key, &bundles, days, now())?;
            for a in &r.added {
                println!("added {a}");
            }
            println!("index.json: serial {}, expires in {days} days", r.serial);
        }
        _ => {
            eprint!("{USAGE}");
            return Ok(ExitCode::from(2));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn expires_days(args: &Args) -> Result<u64, String> {
    args.get("expires-days")
        .map_or(Ok(90), str::parse)
        .map_err(|_| "--expires-days: a number of days".into())
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970")
        .as_secs()
}

fn create(path: &str, text: &str, mode: u32) -> Result<(), String> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(mode);
    #[cfg(not(unix))]
    let _ = mode;
    let mut f = options.open(path).map_err(|e| format!("{path}: {e}"))?;
    f.write_all(text.as_bytes())
        .map_err(|e| format!("{path}: {e}"))
}

/// Everything for the targets of a release: each service once per Rust
/// target, each target's modules, its bundle, checked.
fn build(p: &Project, release: &str, which: &str, native: bool) -> Result<(), String> {
    let list = openwrt::resolve(p, release, which)?;
    let mut rusts: Vec<&str> = list.iter().map(|t| t.rust.as_str()).collect();
    rusts.sort_unstable();
    rusts.dedup();
    for rust in rusts {
        eprintln!("== {} {rust}", p.service);
        docker::build_service(p, rust, native)?;
    }
    for t in &list {
        if !p.modules.is_empty() {
            docker::build_modules(p, &t.openwrt, &t.target, native)?;
        }
        let out = make::bundle(p, t)?;
        let info = bundle::check(&read(&out)?)?;
        eprintln!("== {}: ok, {info}", out.display());
    }
    Ok(())
}
