#![cfg(unix)]

use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

use tempfile::TempDir;

fn run(command: &mut Command) -> Output {
    command.output().expect("test command should start")
}

fn git(repository: &Path, arguments: &[&str]) {
    let output = run(Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments));
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn executable(path: &Path, source: &str) {
    fs::write(path, source).expect("fake executable should be written");
    let mut permissions = fs::metadata(path)
        .expect("fake executable metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("fake executable should be executable");
}

fn publisher(project: &Path, fake_bin: &Path, cross_log: &Path, arguments: &[&str]) -> Output {
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        env::var("PATH").expect("PATH should be available")
    );
    run(Command::new(env!("CARGO_BIN_EXE_rust_realease"))
        .arg(project)
        .args(arguments)
        .env("PATH", path)
        .env("CROSS_LOG", cross_log)
        .env_remove("GITEA_TOKEN"))
}

fn assert_success(output: &Output) -> String {
    assert!(
        output.status.success(),
        "publisher failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn compile_only_reuses_clean_commit_and_rebuilds_changed_or_forced_inputs() {
    let temporary = TempDir::new().expect("temporary directory should be created");
    let project = temporary.path().join("project");
    let fake_bin = temporary.path().join("bin");
    let cross_log = temporary.path().join("cross.log");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(&fake_bin).unwrap();
    fs::write(
        project.join("Cargo.toml"),
        "[package]\n\
         name = \"demo\"\n\
         version = \"0.1.0\"\n\
         edition = \"2021\"\n\
         repository = \"https://example.com/team/demo\"\n\
         [package.metadata.binstall]\n\
         pkg-url = \"{ repo }/releases/download/v{ version }/{ name }-{ target }-v{ version }.tgz\"\n\
         pkg-fmt = \"tgz\"\n\
         bin-dir = \"{ name }-{ target }-v{ version }/{ bin }{ binary-ext }\"\n",
    )
    .unwrap();
    fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(project.join(".gitignore"), "/target/\n/dist/\n").unwrap();
    fs::write(
        project.join("rust-release.toml"),
        "[build]\ntargets = [\"x86_64-unknown-linux-musl\"]\n",
    )
    .unwrap();
    executable(&fake_bin.join("docker"), "#!/bin/sh\n[ \"$1\" = info ]\n");
    executable(&fake_bin.join("cargo-binstall"), "#!/bin/sh\nexit 0\n");
    executable(
        &fake_bin.join("cargo-cross"),
        "#!/bin/sh\n\
         set -eu\n\
         target=\n\
         target_dir=\n\
         profile=debug\n\
         binaries=\n\
         while [ \"$#\" -gt 0 ]; do\n\
           case \"$1\" in\n\
             --target) shift; target=$1 ;;\n\
             --target-dir) shift; target_dir=$1 ;;\n\
             --release) profile=release ;;\n\
             --profile) shift; profile=$1 ;;\n\
             --bin) shift; binaries=\"$binaries $1\" ;;\n\
           esac\n\
           shift\n\
         done\n\
         output_dir=$target_dir/$target/$profile\n\
         mkdir -p \"$output_dir\"\n\
         for binary in $binaries; do printf 'compiled %s' \"$binary\" >\"$output_dir/$binary\"; done\n\
         printf 'cross invoked\\n' >>\"$CROSS_LOG\"\n",
    );

    git(&project, &["init", "--quiet"]);
    git(&project, &["config", "user.name", "Release Test"]);
    git(&project, &["config", "user.email", "release@example.com"]);
    git(&project, &["config", "commit.gpgsign", "false"]);
    let lock = run(Command::new("cargo")
        .args(["generate-lockfile", "--manifest-path"])
        .arg(project.join("Cargo.toml")));
    assert!(lock.status.success());
    git(&project, &["add", "."]);
    git(&project, &["commit", "--quiet", "-m", "initial"]);
    git(&project, &["tag", "v0.1.0"]);

    let dry_run = assert_success(&publisher(&project, &fake_bin, &cross_log, &["--dry-run"]));
    assert!(dry_run.contains("Dry run complete"));
    assert!(!cross_log.exists());

    let first = assert_success(&publisher(
        &project,
        &fake_bin,
        &cross_log,
        &["--compile-only"],
    ));
    assert!(first.contains("Cross builds complete"));
    assert_eq!(fs::read_to_string(&cross_log).unwrap().lines().count(), 1);

    let second = assert_success(&publisher(
        &project,
        &fake_bin,
        &cross_log,
        &["--compile-only"],
    ));
    assert!(second.contains("Reusing completed release build"));
    assert_eq!(fs::read_to_string(&cross_log).unwrap().lines().count(), 1);

    fs::write(
        project.join("src/main.rs"),
        "fn main() { println!(\"changed\"); }\n",
    )
    .unwrap();
    let dirty_publication = publisher(&project, &fake_bin, &cross_log, &[]);
    assert!(!dirty_publication.status.success());
    assert!(String::from_utf8_lossy(&dirty_publication.stderr)
        .contains("Publication requires a clean checkout"));
    assert_success(&publisher(
        &project,
        &fake_bin,
        &cross_log,
        &["--compile-only"],
    ));
    assert_eq!(fs::read_to_string(&cross_log).unwrap().lines().count(), 2);

    git(&project, &["add", "src/main.rs"]);
    git(&project, &["commit", "--quiet", "-m", "change source"]);
    let changed = assert_success(&publisher(
        &project,
        &fake_bin,
        &cross_log,
        &["--compile-only"],
    ));
    assert!(changed.contains("Cross builds complete"));
    assert_eq!(fs::read_to_string(&cross_log).unwrap().lines().count(), 3);

    assert_success(&publisher(
        &project,
        &fake_bin,
        &cross_log,
        &["--compile-only", "--recompile"],
    ));
    assert_eq!(fs::read_to_string(&cross_log).unwrap().lines().count(), 4);
}

#[test]
fn missing_message_file_is_reported_before_project_acquisition() {
    let temporary = TempDir::new().unwrap();
    let output = run(Command::new(env!("CARGO_BIN_EXE_rust_realease"))
        .arg("missing-project")
        .args(["--message-file", "missing-notes.md"])
        .current_dir(temporary.path()));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read release message file"));
}

#[test]
fn bundle_cli_uses_reported_paths_and_reruns_preparation_on_cache_hits() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("workspace é");
    let bin = tmp.path().join("tools");
    let log = tmp.path().join("cross.log");
    fs::create_dir_all(root.join("member/src")).unwrap();
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::create_dir(&bin).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers=['member']\nresolver='2'\n",
    )
    .unwrap();
    fs::write(
        root.join("member/Cargo.toml"),
        r#"[package]
name='demo'
version='0.1.0'
edition='2021'
repository='https://example.com/team/demo'
[package.metadata.binstall]
pkg-url='{ repo }/releases/download/v{ version }/{ name }-{ target }-v{ version }.tgz'
pkg-fmt='tgz'
bin-dir='{ name }-{ target }-v{ version }/{ bin }{ binary-ext }'
"#,
    )
    .unwrap();
    fs::write(root.join("member/src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        root.join(".cargo/config.toml"),
        "[build]\ntarget-dir='custom build dir'\n",
    )
    .unwrap();
    fs::write(root.join(".gitignore"), "/custom build dir/\n/dist/\n").unwrap();
    fs::write(
        root.join("rust-release.toml"),
        r#"[build]
targets=['x86_64-unknown-linux-gnu']
[[package.bundles]]
name='portable'
formats={x86_64-unknown-linux-gnu='tgz'}
archive-name='{name}-{bundle}-{target}-v{version}'
root='{name}-{bundle}-{target}-v{version}'
cargo-binaries='all'
[package.bundles.prepare]
command=['python3','prepare.py','{context-file}']
inputs=['prepare.py']
timeout-seconds=10
required-destinations=['NOTICE']
"#,
    )
    .unwrap();
    fs::write(root.join("member/prepare.py"),r#"import sys,json,pathlib,hashlib
c=json.loads(pathlib.Path(sys.argv[1]).read_text())
assert pathlib.Path(c['package_root']).name=='member'
assert pathlib.Path(c['workspace_root']).name=='workspace é'
assert 'actual output é' in c['binaries']['demo']
assert pathlib.Path(c['binaries']['demo']).is_file()
cache=pathlib.Path(c['cache_directory'])
with (cache/'calls').open('a') as f: f.write(str(c['source_dirty'])+'\n')
out=pathlib.Path(c['output_directory'])/'notice'
out.write_bytes(b'notice')
pathlib.Path(c['manifest_path']).write_text(json.dumps(dict(schema_version=1,target=c['target'],files=[dict(source='notice',destination='NOTICE',kind='file',sha256=hashlib.sha256(b'notice').hexdigest())])))
"#).unwrap();
    executable(&bin.join("docker"), "#!/bin/sh\nexit 0\n");
    executable(&bin.join("cargo-binstall"), "#!/bin/sh\nexit 0\n");
    executable(
        &bin.join("cross"),
        r#"#!/usr/bin/env python3
import sys,pathlib,shutil,json,os
a=sys.argv
assert '--message-format=json-render-diagnostics' in a
out=pathlib.Path(a[a.index('--target-dir')+1])/'actual output é'
out.mkdir(parents=True)
binary=out/'demo'
shutil.copyfile('/bin/true',binary)
print(json.dumps(dict(reason='compiler-artifact',target=dict(name='demo',kind=['bin']),executable="/target/actual output é/demo")))
with open(os.environ['CROSS_LOG'],'a') as f: f.write('build\n')
"#,
    );
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "Test"]);
    git(&root, &["config", "user.email", "test@example.com"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    assert!(run(Command::new("cargo")
        .arg("generate-lockfile")
        .current_dir(&root))
    .status
    .success());
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "fixture"]);
    git(&root, &["tag", "v0.1.0"]);
    assert_success(&publisher(
        &root,
        &bin,
        &log,
        &["--package", "demo", "--dry-run"],
    ));
    assert!(!root.join("dist").exists());
    assert!(!log.exists());
    for _ in 0..2 {
        assert_success(&publisher(
            &root,
            &bin,
            &log,
            &["--package", "demo", "--compile-only", "--skip-tests"],
        ));
    }
    assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 1);
    let calls = root
        .join("custom build dir/rust-release-bundle-cache/portable/x86_64-unknown-linux-gnu/calls");
    assert_eq!(fs::read_to_string(&calls).unwrap(), "False\nFalse\n");
    fs::write(
        root.join("member/src/main.rs"),
        "fn main() {println!(\"dirty\");}\n",
    )
    .unwrap();
    assert_success(&publisher(
        &root,
        &bin,
        &log,
        &[
            "--package",
            "demo",
            "--stop-after",
            "package",
            "--skip-tests",
        ],
    ));
    assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 2);
    assert!(fs::read_to_string(calls).unwrap().ends_with("True\n"));
    // Cancellation is delivered to a disposable publisher process, not the test runner.
    fs::write(
        root.join("member/prepare.py"),
        "import os,signal,time\nos.kill(os.getppid(), signal.SIGTERM)\ntime.sleep(5)\n",
    )
    .unwrap();
    let cancelled = publisher(
        &root,
        &bin,
        &log,
        &["--package", "demo", "--compile-only", "--skip-tests"],
    );
    assert!(!cancelled.status.success());
    assert!(String::from_utf8_lossy(&cancelled.stderr).contains("cancelled"));
}

#[test]
fn mixed_cross_and_apple_compile_only_routes_and_packages_eight_assets() {
    let temporary = TempDir::new().unwrap();
    let project = temporary.path().join("mixed checkout with spaces");
    let tools = temporary.path().join("tools");
    let log = temporary.path().join("routes.log");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir(&tools).unwrap();
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='2.3.4'\nedition='2021'\nrepository='https://example.com/team/demo'\n[features]\nserial=[]\n[package.metadata.binstall]\npkg-url='{ repo }/releases/download/{ version }/{ name }-{ target }-v{ version }.tgz'\npkg-fmt='tgz'\nbin-dir='{ name }-{ target }-v{ version }/{ bin }{ binary-ext }'\n",
    )
    .unwrap();
    fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(project.join(".gitignore"), "/target/\n/dist/\n").unwrap();
    fs::write(project.join("release.Cross.toml"), "[build]\n").unwrap();
    fs::write(
        project.join("rust-release.toml"),
        "[release]\ntag-prefix=''\n[build]\ntargets=['x86_64-unknown-linux-gnu','x86_64-pc-windows-gnu','aarch64-apple-darwin','x86_64-apple-darwin']\nprofile='release'\nlocked=true\nfeatures=['serial']\n[package]\nformat='both'\n",
    )
    .unwrap();
    executable(&tools.join("cargo-binstall"), "#!/bin/sh\nexit 0\n");
    executable(
        &tools.join("cross"),
        "#!/bin/sh\nset -eu\n[ -n \"${CROSS_CONFIG:-}\" ]\ntarget=\ntarget_dir=\nwhile [ $# -gt 0 ]; do case $1 in --target) shift; target=$1;; --target-dir) shift; target_dir=$1;; --features) shift; [ \"$1\" = serial ];; esac; shift; done\ncase $target in *windows*) suffix=.exe;; *) suffix=;; esac\nmkdir -p \"$target_dir/$target/release\"\nprintf cross >\"$target_dir/$target/release/demo$suffix\"\nprintf 'cross:%s\\n' \"$target\" >>\"$CROSS_LOG\"\n",
    );
    executable(
        &tools.join("docker"),
        r#"#!/usr/bin/env python3
import json, os, pathlib, struct, sys
if sys.argv[1:] == ['info']:
    raise SystemExit(0)
assert 'RELEASE_TEST_SECRET' not in os.environ
if os.environ.get('ENGINE_STATUS'):
    raise SystemExit(int(os.environ['ENGINE_STATUS']))
args=sys.argv[1:]
assert args[0:2] == ['run','--rm'] and '-it' not in args
assert not any('sock' in arg for arg in args)
assert '--userns=keep-id' not in args
assert 'serial' in args and '--locked' in args and '--release' in args
mount=next(args[i+1] for i,arg in enumerate(args) if arg=='-v' and args[i+1].endswith(':/output'))
host=pathlib.Path(mount[:-len(':/output')])
target=args[args.index('--target')+1]
cpu=0x0100000c if target.startswith('aarch64') else 0x01000007
dependency=b'/usr/lib/libSystem.B.dylib\0'
dylib_size=(24+len(dependency)+7)//8*8
data=bytearray(32+24+dylib_size)
data[:4]=bytes.fromhex('cffaedfe')
struct.pack_into('<I',data,4,cpu); struct.pack_into('<I',data,12,2)
struct.pack_into('<II',data,16,2,24+dylib_size)
struct.pack_into('<IIIIII',data,32,0x32,24,1,13<<16,0,0)
struct.pack_into('<IIIIII',data,56,0xc,dylib_size,24,0,0,0)
data[80:80+len(dependency)]=dependency
binary=host/'target'/target/'release'/'demo'
binary.parent.mkdir(parents=True)
binary.write_bytes(data); binary.chmod(0o755)
print(json.dumps({'reason':'compiler-artifact','target':{'name':'demo','kind':['bin']},'executable':f'/output/target/{target}/release/demo'}))
with open(os.environ['CROSS_LOG'],'a') as stream: stream.write('zig:'+target+'\n')
"#,
    );
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.name", "Test"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "commit.gpgsign", "false"]);
    assert!(run(Command::new("cargo")
        .arg("generate-lockfile")
        .current_dir(&project))
    .status
    .success());
    git(&project, &["add", "."]);
    git(&project, &["commit", "-qm", "fixture"]);
    git(&project, &["tag", "2.3.4"]);

    let path = format!("{}:{}", tools.display(), env::var("PATH").unwrap());
    let dry = run(Command::new(env!("CARGO_BIN_EXE_rust_realease"))
        .arg(&project)
        .args(["--dry-run"])
        .env("PATH", &path)
        .env("CROSS_CONTAINER_ENGINE", "docker")
        .env("CROSS_CONFIG", project.join("release.Cross.toml"))
        .env("CROSS_LOG", &log));
    let dry_stdout = assert_success(&dry);
    assert!(dry_stdout.contains("cargo-zigbuild container"));
    assert!(dry_stdout.contains("build backend: Cross"));
    assert!(!log.exists());

    let output = run(Command::new(env!("CARGO_BIN_EXE_rust_realease"))
        .arg(&project)
        .args(["--compile-only", "--skip-tests"])
        .env("PATH", &path)
        .env("CROSS_CONTAINER_ENGINE", "docker")
        .env("CROSS_CONFIG", project.join("release.Cross.toml"))
        .env("CROSS_LOG", &log)
        .env("RELEASE_TEST_SECRET", "must-not-reach-compilers")
        .env_remove("GITEA_TOKEN"));
    let stdout = assert_success(&output);
    assert!(stdout.contains("runtime skipped (no compatible macOS runner)"));
    let routes = fs::read_to_string(&log).unwrap();
    assert_eq!(
        routes
            .lines()
            .filter(|line| line.starts_with("cross:"))
            .count(),
        2
    );
    assert_eq!(
        routes
            .lines()
            .filter(|line| line.starts_with("zig:"))
            .count(),
        2
    );
    let artifact_dir = fs::read_dir(project.join("dist"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.is_dir() && path.join("SHA256SUMS").is_file())
        .unwrap();
    let checksums = fs::read_to_string(artifact_dir.join("SHA256SUMS")).unwrap();
    assert_eq!(checksums.lines().count(), 8);
    for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
        assert!(artifact_dir
            .join(format!("demo-{target}-v2.3.4.tgz"))
            .is_file());
        let binary = artifact_dir.join(format!("demo-{target}-v2.3.4"));
        assert!(binary.is_file());
        assert_ne!(
            fs::metadata(binary).unwrap().permissions().mode() & 0o111,
            0
        );
    }

    let cached = run(Command::new(env!("CARGO_BIN_EXE_rust_realease"))
        .arg(&project)
        .args(["--compile-only", "--skip-tests"])
        .env("PATH", path)
        .env("CROSS_CONTAINER_ENGINE", "docker")
        .env("CROSS_CONFIG", project.join("release.Cross.toml"))
        .env("CROSS_LOG", &log));
    assert!(assert_success(&cached).contains("Reusing completed release build"));
    assert_eq!(fs::read_to_string(log).unwrap().lines().count(), 4);

    let failed = run(Command::new(env!("CARGO_BIN_EXE_rust_realease"))
        .arg(&project)
        .args(["--compile-only", "--skip-tests", "--recompile"])
        .env(
            "PATH",
            format!("{}:{}", tools.display(), env::var("PATH").unwrap()),
        )
        .env("CROSS_CONTAINER_ENGINE", "docker")
        .env("CROSS_CONFIG", project.join("release.Cross.toml"))
        .env("CROSS_LOG", temporary.path().join("failed.log"))
        .env("ENGINE_STATUS", "42"));
    assert!(!failed.status.success());
    assert!(!String::from_utf8_lossy(&failed.stdout).contains("Packaged"));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("exit status: 42"));
}
