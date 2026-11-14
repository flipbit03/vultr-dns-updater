//! The real binary against wiremock: Vultr through `VULTR_DNS_UPDATER_API_URL`,
//! the public-IP services through `ip_check_urls` in a temp config. Nothing
//! here reaches the internet.

use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use serde_json::{Value, json};
use wiremock::matchers::{
    body_json, header, method, path, path_regex, query_param, query_param_is_missing,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

const IP: &str = "45.77.1.2";
const KEY: &str = "test-key-do-not-print-0123456789";

struct Env {
    dir: tempfile::TempDir,
    vultr: MockServer,
    ipsvc: MockServer,
}

impl Env {
    async fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            vultr: MockServer::start().await,
            ipsvc: MockServer::start().await,
        }
    }

    fn home(&self) -> &Path {
        self.dir.path()
    }

    /// The binary, isolated: temp HOME and cwd, no key from this machine,
    /// Vultr and GitHub pointed at mocks / nowhere.
    fn cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("vultr-dns").unwrap();
        cmd.current_dir(self.home())
            .env("HOME", self.home())
            .env("XDG_CACHE_HOME", self.home().join(".cache"))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("VULTR_API_KEY")
            .env("NO_COLOR", "1")
            .env("VULTR_DNS_UPDATER_API_URL", self.vultr.uri())
            .env("VULTR_DNS_UPDATER_GITHUB_API_URL", "http://127.0.0.1:9")
            .env("VULTR_DNS_UPDATER_RELEASES_URL", "http://127.0.0.1:9");
        cmd
    }

    fn ip_url(&self, p: &str) -> String {
        format!("{}{p}", self.ipsvc.uri())
    }

    /// Writes a config with the key, these targets and one IP service.
    fn config(&self, targets: &[(&str, &str, u32)]) -> PathBuf {
        let mut text = format!(
            "api_key = \"{KEY}\"\nip_check_urls = [\"{}\"]\n",
            self.ip_url("/ip")
        );
        for (domain, sub, ttl) in targets {
            text.push_str(&format!(
                "\n[[targets]]\ndomain = \"{domain}\"\nsubdomain = \"{sub}\"\nttl = {ttl}\n"
            ));
        }
        let p = self.home().join("cfg.toml");
        std::fs::write(&p, text).unwrap();
        p
    }

    async fn ip_answers(&self, p: &str, status: u16, body: &str) {
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .mount(&self.ipsvc)
            .await;
    }

    async fn records(&self, domain: &str, records: Value) {
        Mock::given(method("GET"))
            .and(path(format!("/domains/{domain}/records")))
            .and(header("authorization", format!("Bearer {KEY}").as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "records": records,
                "meta": {"total": 1, "links": {"next": "", "prev": ""}}
            })))
            .mount(&self.vultr)
            .await;
    }

    async fn no_writes(&self) {
        for m in ["POST", "PATCH", "DELETE", "PUT"] {
            Mock::given(method(m))
                .respond_with(ResponseTemplate::new(500))
                .expect(0)
                .named(format!("no {m}"))
                .mount(&self.vultr)
                .await;
        }
    }
}

fn record(id: &str, name: &str, data: &str, ttl: u32) -> Value {
    json!({"id": id, "type": "A", "name": name, "data": data, "priority": 0, "ttl": ttl})
}

/// Runs a command without blocking the mock servers' runtime.
async fn run(mut cmd: Command) -> (String, String, Option<i32>) {
    let out: Output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
        out.status.code(),
    )
}

async fn update(env: &Env, cfg: &Path, extra: &[&str]) -> (String, String, Option<i32>) {
    let mut cmd = env.cmd();
    cmd.arg("update").arg("--config").arg(cfg).args(extra);
    run(cmd).await
}

#[tokio::test]
async fn up_to_date_run_prints_the_journal_lines_exactly() {
    let env = Env::new().await;
    let cfg = env.config(&[("flipbit03.com", "home", 60)]);
    env.ip_answers("/ip", 200, &format!("{IP}\n")).await;
    env.records("flipbit03.com", json!([record("r1", "home", IP, 60)]))
        .await;
    env.no_writes().await;

    let (stdout, stderr, code) = update(&env, &cfg, &[]).await;
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        format!(
            "Info: Current public IP: {IP}\n\
             \n\
             Processing: home.flipbit03.com\n  \
             Current: {IP} (TTL: 60)\n\
             Info:   home.flipbit03.com is already up to date\n"
        )
    );
    assert_eq!(stderr, "");
}

#[tokio::test]
async fn creates_a_missing_record() {
    let env = Env::new().await;
    let cfg = env.config(&[("example.com", "home", 120)]);
    env.ip_answers("/ip", 200, IP).await;
    env.records("example.com", json!([record("r9", "www", "9.9.9.9", 60)]))
        .await;
    Mock::given(method("POST"))
        .and(path("/domains/example.com/records"))
        .and(body_json(json!({
            "name": "home", "type": "A", "data": IP, "ttl": 120, "priority": 0
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "record": record("new", "home", IP, 120)
        })))
        .expect(1)
        .mount(&env.vultr)
        .await;

    let (stdout, stderr, code) = update(&env, &cfg, &[]).await;
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains("  Current: No record exists\n"), "{stdout}");
    assert!(
        stdout.contains(&format!("Success:   Created home.example.com -> {IP}\n")),
        "{stdout}"
    );
}

#[tokio::test]
async fn updates_a_changed_ip_with_data_and_ttl_only() {
    let env = Env::new().await;
    let cfg = env.config(&[("example.com", "home", 60)]);
    env.ip_answers("/ip", 200, IP).await;
    env.records("example.com", json!([record("r1", "home", "1.1.1.1", 60)]))
        .await;
    Mock::given(method("PATCH"))
        .and(path("/domains/example.com/records/r1"))
        .and(body_json(json!({"data": IP, "ttl": 60})))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&env.vultr)
        .await;

    let (stdout, stderr, code) = update(&env, &cfg, &[]).await;
    assert_eq!(code, Some(0), "{stderr}");
    assert!(
        stdout.contains("  Current: 1.1.1.1 (TTL: 60)\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "Success:   Updated home.example.com: 1.1.1.1 -> {IP}\n"
        )),
        "{stdout}"
    );
}

#[tokio::test]
async fn ttl_only_change_says_so() {
    let env = Env::new().await;
    let cfg = env.config(&[("example.com", "home", 60)]);
    env.ip_answers("/ip", 200, IP).await;
    env.records("example.com", json!([record("r1", "home", IP, 300)]))
        .await;
    Mock::given(method("PATCH"))
        .and(path("/domains/example.com/records/r1"))
        .and(body_json(json!({"data": IP, "ttl": 60})))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&env.vultr)
        .await;

    let (stdout, _, code) = update(&env, &cfg, &[]).await;
    assert_eq!(code, Some(0));
    assert!(
        stdout.contains(&format!(
            "Success:   Updated home.example.com: {IP} -> {IP} (TTL: 300 -> 60)\n"
        )),
        "{stdout}"
    );
}

#[tokio::test]
async fn force_writes_even_when_up_to_date() {
    let env = Env::new().await;
    let cfg = env.config(&[("example.com", "home", 60)]);
    env.ip_answers("/ip", 200, IP).await;
    env.records("example.com", json!([record("r1", "home", IP, 60)]))
        .await;
    Mock::given(method("PATCH"))
        .and(path("/domains/example.com/records/r1"))
        .and(body_json(json!({"data": IP, "ttl": 60})))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&env.vultr)
        .await;

    let (stdout, _, code) = update(&env, &cfg, &["--force"]).await;
    assert_eq!(code, Some(0));
    assert!(
        stdout.contains(&format!(
            "Success:   Updated home.example.com: {IP} -> {IP}\n"
        )),
        "{stdout}"
    );
}

#[tokio::test]
async fn dry_run_writes_nothing() {
    let env = Env::new().await;
    let cfg = env.config(&[("example.com", "home", 60), ("example.com", "work", 60)]);
    env.ip_answers("/ip", 200, IP).await;
    env.records("example.com", json!([record("r1", "home", "1.1.1.1", 60)]))
        .await;
    env.no_writes().await;

    let (stdout, stderr, code) = update(&env, &cfg, &["--dry-run"]).await;
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        format!(
            "Info: Current public IP: {IP}\n\
             \n\
             DRY RUN MODE - No changes will be made\n\
             \n\
             \n\
             Processing: home.example.com\n  \
             Current: 1.1.1.1 (TTL: 60)\n  \
             Would update: 1.1.1.1 -> {IP}\n\
             \n\
             Processing: work.example.com\n  \
             Current: No record exists\n  \
             Would create: work A {IP}\n"
        )
    );
}

#[tokio::test]
async fn custom_ip_skips_detection() {
    let env = Env::new().await;
    let cfg = env.config(&[("example.com", "home", 60)]);
    env.records("example.com", json!([record("r1", "home", "10.0.0.5", 60)]))
        .await;
    env.no_writes().await;

    let (stdout, _, code) = update(&env, &cfg, &["--ip", "10.0.0.5"]).await;
    assert_eq!(code, Some(0));
    assert!(
        stdout.starts_with("Info: Using custom IP: 10.0.0.5\n"),
        "{stdout}"
    );
    assert!(stdout.contains("is already up to date"));

    let (_, stderr, code) = update(&env, &cfg, &["--ip", "not-an-ip"]).await;
    assert_eq!(code, Some(2), "{stderr}");
}

#[tokio::test]
async fn follows_the_cursor_to_page_two() {
    let env = Env::new().await;
    let cfg = env.config(&[("example.com", "home", 60)]);
    env.ip_answers("/ip", 200, IP).await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com/records"))
        .and(query_param("per_page", "500"))
        .and(query_param_is_missing("cursor"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [record("r1", "www", "9.9.9.9", 60)],
            "meta": {"total": 2, "links": {"next": "page2", "prev": ""}}
        })))
        .expect(1)
        .mount(&env.vultr)
        .await;
    Mock::given(method("GET"))
        .and(path("/domains/example.com/records"))
        .and(query_param("per_page", "500"))
        .and(query_param("cursor", "page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "records": [record("r2", "home", IP, 60)],
            "meta": {"total": 2, "links": {"next": "", "prev": "page1"}}
        })))
        .expect(1)
        .mount(&env.vultr)
        .await;
    env.no_writes().await;

    let (stdout, stderr, code) = update(&env, &cfg, &[]).await;
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains("is already up to date"), "{stdout}");
}

#[tokio::test]
async fn ip_detection_falls_through_bad_answers() {
    let env = Env::new().await;
    env.ip_answers("/a", 500, "oops").await;
    env.ip_answers("/b", 200, "2001:db8::1").await;
    env.ip_answers("/c", 200, "<html>captive portal</html>")
        .await;
    env.ip_answers("/d", 200, "192.168.0.10").await;
    env.ip_answers("/e", 200, &format!(" {IP}\n")).await;
    let urls: Vec<String> = ["/a", "/b", "/c", "/d", "/e"]
        .iter()
        .map(|p| format!("\"{}\"", env.ip_url(p)))
        .collect();
    let cfg = env.home().join("ip.toml");
    std::fs::write(&cfg, format!("ip_check_urls = [{}]\n", urls.join(", "))).unwrap();

    let mut cmd = env.cmd();
    cmd.args(["get-ip", "-c"]).arg(&cfg);
    let (stdout, stderr, code) = run(cmd).await;
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, format!("{IP}\n"));
}

#[tokio::test]
async fn ip_detection_all_failing_lists_each_url() {
    let env = Env::new().await;
    env.ip_answers("/a", 503, "").await;
    env.ip_answers("/b", 200, "nope").await;
    let urls = [
        env.ip_url("/a"),
        env.ip_url("/b"),
        "http://127.0.0.1:9/x".to_string(),
    ];
    let cfg = env.home().join("ip.toml");
    let list: Vec<String> = urls.iter().map(|u| format!("\"{u}\"")).collect();
    std::fs::write(
        &cfg,
        format!(
            "api_key = \"{KEY}\"\nip_check_urls = [{}]\n[[targets]]\ndomain = \"example.com\"\nsubdomain = \"home\"\n",
            list.join(", ")
        ),
    )
    .unwrap();
    env.no_writes().await;

    let (stdout, stderr, code) = update(&env, &cfg, &[]).await;
    assert_eq!(code, Some(1));
    assert_eq!(stdout, "");
    assert!(
        stderr.starts_with("Error: Failed to detect public IP. Errors:\n"),
        "{stderr}"
    );
    let lines: Vec<&str> = stderr.lines().skip(1).collect();
    assert_eq!(lines.len(), 3, "{stderr}");
    assert!(
        lines[0].starts_with(&format!("  - {}: HTTP 503", urls[0])),
        "{stderr}"
    );
    assert!(
        lines[1].starts_with(&format!("  - {}: Invalid IP format: nope", urls[1])),
        "{stderr}"
    );
    assert!(
        lines[2].starts_with(&format!("  - {}: ", urls[2])),
        "{stderr}"
    );
}

#[tokio::test]
async fn one_failing_domain_doesnt_stop_the_others() {
    let env = Env::new().await;
    let cfg = env.config(&[("bad.com", "home", 60), ("good.com", "home", 60)]);
    env.ip_answers("/ip", 200, IP).await;
    Mock::given(method("GET"))
        .and(path("/domains/bad.com/records"))
        .respond_with(
            ResponseTemplate::new(500).set_body_json(json!({"error": "boom", "status": 500})),
        )
        .mount(&env.vultr)
        .await;
    env.records("good.com", json!([record("g1", "home", "1.1.1.1", 60)]))
        .await;
    Mock::given(method("PATCH"))
        .and(path("/domains/good.com/records/g1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&env.vultr)
        .await;

    let (stdout, stderr, code) = update(&env, &cfg, &[]).await;
    assert_eq!(code, Some(1));
    assert_eq!(stderr, "Error: Vultr API error (500): boom\n");
    assert!(stdout.contains("Processing: home.bad.com"), "{stdout}");
    assert!(
        stdout.contains(&format!(
            "Success:   Updated home.good.com: 1.1.1.1 -> {IP}\n"
        )),
        "{stdout}"
    );
}

#[tokio::test]
async fn bad_key_is_401_and_never_printed() {
    let env = Env::new().await;
    let cfg = env.config(&[("example.com", "home", 60)]);
    env.ip_answers("/ip", 200, IP).await;
    Mock::given(path_regex(".*"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"error": "Invalid API token.", "status": 401})),
        )
        .mount(&env.vultr)
        .await;

    let (stdout, stderr, code) = update(&env, &cfg, &[]).await;
    assert_eq!(code, Some(1));
    assert!(
        stderr.contains("Error: Vultr API error (401): Invalid API token."),
        "{stderr}"
    );
    assert!(!stdout.contains(KEY) && !stderr.contains(KEY));

    let mut cmd = env.cmd();
    cmd.args(["list-domains", "--api-key", KEY]);
    let (stdout, stderr, code) = run(cmd).await;
    assert_eq!(code, Some(1));
    assert!(
        stderr.starts_with("Error: Vultr API error (401): Invalid API token."),
        "{stderr}"
    );
    assert!(!stdout.contains(KEY) && !stderr.contains(KEY));
}

#[tokio::test]
async fn network_errors_name_the_url_without_a_traceback() {
    let env = Env::new().await;
    let cfg = env.config(&[("example.com", "home", 60)]);
    env.ip_answers("/ip", 200, IP).await;
    let mut cmd = env.cmd();
    cmd.env("VULTR_DNS_UPDATER_API_URL", "http://127.0.0.1:9/v2")
        .args(["update", "-c"])
        .arg(&cfg);
    let (_, stderr, code) = run(cmd).await;
    assert_eq!(code, Some(1));
    assert!(stderr.starts_with("Error: error sending request for url (http://127.0.0.1:9/v2/domains/example.com/records"), "{stderr}");
    assert!(!stderr.contains(KEY));
}

#[tokio::test]
async fn duplicate_records_warn() {
    let env = Env::new().await;
    let cfg = env.config(&[("example.com", "home", 60)]);
    env.ip_answers("/ip", 200, IP).await;
    env.records(
        "example.com",
        json!([
            record("r1", "home", IP, 60),
            record("r2", "home", "1.1.1.1", 60)
        ]),
    )
    .await;
    env.no_writes().await;

    let (_, stderr, code) = update(&env, &cfg, &[]).await;
    assert_eq!(code, Some(0));
    assert_eq!(
        stderr,
        "Warning:   2 A records named \"home\" in example.com; updating r1 only\n"
    );
}

#[tokio::test]
async fn flags_give_the_target_and_env_key_beats_the_file() {
    let env = Env::new().await;
    let cfg = env.config(&[("ignored.com", "nope", 60)]);
    env.ip_answers("/ip", 200, IP).await;
    Mock::given(method("GET"))
        .and(path("/domains/flags.com/records"))
        .and(header("authorization", "Bearer from-env"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"records": [], "meta": {}})))
        .expect(1)
        .mount(&env.vultr)
        .await;
    Mock::given(method("POST"))
        .and(path("/domains/flags.com/records"))
        .and(header("authorization", "Bearer from-env"))
        .and(body_json(
            json!({"name": "", "type": "A", "data": IP, "ttl": 300, "priority": 0}),
        ))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({"record": record("n", "", IP, 300)})),
        )
        .expect(1)
        .mount(&env.vultr)
        .await;

    let mut cmd = env.cmd();
    cmd.env("VULTR_API_KEY", "from-env")
        .args(["update", "-c"])
        .arg(&cfg)
        .args(["-d", "flags.com", "-s", "", "-t", "300"]);
    let (stdout, stderr, code) = run(cmd).await;
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains("Processing: flags.com\n"), "{stdout}");
    assert!(
        stdout.contains(&format!("Success:   Created flags.com -> {IP}")),
        "{stdout}"
    );
}

#[tokio::test]
async fn domain_without_subdomain_is_a_usage_error() {
    let env = Env::new().await;
    let cfg = env.config(&[("example.com", "home", 60)]);
    let (_, stderr, code) = update(&env, &cfg, &["-d", "example.com"]).await;
    assert_eq!(code, Some(2), "{stderr}");
    let mut cmd = env.cmd();
    cmd.args(["update", "--config", "/does/not/exist"]);
    assert_eq!(run(cmd).await.2, Some(2));
}

#[tokio::test]
async fn missing_key_and_missing_config() {
    let env = Env::new().await;
    let cfg = env.home().join("nokey.toml");
    std::fs::write(&cfg, "[[targets]]\ndomain = \"a.com\"\nsubdomain = \"h\"\n").unwrap();
    let (_, stderr, code) = update(&env, &cfg, &[]).await;
    assert_eq!(code, Some(1));
    assert_eq!(
        stderr,
        "Error: No API key provided. Use --api-key, set VULTR_API_KEY, or add to config\n"
    );

    let mut cmd = env.cmd();
    cmd.arg("update");
    let (_, stderr, code) = run(cmd).await;
    assert_eq!(code, Some(1));
    assert!(
        stderr.starts_with("Error: No configuration file found. Searched:\n"),
        "{stderr}"
    );
    assert!(stderr.contains("vultr-dns init-config"), "{stderr}");
}

#[tokio::test]
async fn search_order_with_xdg_home_and_cwd() {
    let env = Env::new().await;
    // Every domain's list is empty; dry runs show which config won.
    Mock::given(method("GET"))
        .and(path_regex("^/domains/[^/]+/records$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"records": [], "meta": {}})))
        .mount(&env.vultr)
        .await;
    env.no_writes().await;
    let write = |p: PathBuf, domain: &str| {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            format!("api_key = \"{KEY}\"\n[[targets]]\ndomain = \"{domain}\"\nsubdomain = \"h\"\n"),
        )
        .unwrap();
        p
    };
    let xdg = env.home().join("xdg");
    let a = write(xdg.join("vultr-dns-updater/config.toml"), "xdg.com");
    let b = write(
        env.home().join(".config/vultr-dns-updater/config.toml"),
        "dotconfig.com",
    );
    let c = write(env.home().join(".vultr-dns-updater.toml"), "dotfile.com");
    let d = write(env.home().join("vultr-dns-updater.toml"), "cwd.com");

    let winner = |with_xdg: bool| {
        let mut cmd = env.cmd();
        if with_xdg {
            cmd.env("XDG_CONFIG_HOME", &xdg);
        }
        cmd.args(["update", "--dry-run", "--ip", IP]);
        async move {
            let (stdout, stderr, code) = run(cmd).await;
            assert_eq!(code, Some(0), "{stderr}");
            stdout
                .lines()
                .find_map(|l| l.strip_prefix("Processing: h."))
                .unwrap()
                .to_string()
        }
    };
    assert_eq!(winner(true).await, "xdg.com");
    assert_eq!(winner(false).await, "dotconfig.com");
    std::fs::remove_file(a).unwrap();
    assert_eq!(winner(true).await, "dotfile.com");
    std::fs::remove_file(b).unwrap();
    assert_eq!(winner(false).await, "dotfile.com");
    std::fs::remove_file(c).unwrap();
    assert_eq!(winner(false).await, "cwd.com");
    std::fs::remove_file(d).unwrap();
}

#[tokio::test]
async fn unknown_config_key_warns() {
    let env = Env::new().await;
    let cfg = env.home().join("typo.toml");
    std::fs::write(
        &cfg,
        format!(
            "api_key = \"{KEY}\"\n[[targets]]\ndomain = \"a.com\"\nsubdomain = \"h\"\ntll = 300\n"
        ),
    )
    .unwrap();
    env.records("a.com", json!([record("r1", "h", IP, 60)]))
        .await;
    let (_, stderr, code) = update(&env, &cfg, &["--ip", IP]).await;
    assert_eq!(code, Some(0));
    assert_eq!(
        stderr,
        format!(
            "Warning: unknown config key \"targets.0.tll\" in {}\n",
            cfg.display()
        )
    );
}

#[tokio::test]
async fn status_compares_ttl_too() {
    let env = Env::new().await;
    let cfg = env.config(&[
        ("example.com", "ok", 60),
        ("example.com", "ttl", 60),
        ("example.com", "gone", 60),
    ]);
    env.ip_answers("/ip", 200, IP).await;
    env.records(
        "example.com",
        json!([record("r1", "ok", IP, 60), record("r2", "ttl", IP, 300)]),
    )
    .await;
    env.no_writes().await;

    let mut cmd = env.cmd();
    cmd.args(["status", "-c"]).arg(&cfg);
    let (stdout, stderr, code) = run(cmd).await;
    assert_eq!(code, Some(0), "{stderr}");
    assert!(
        stdout.starts_with(&format!("Current Public IP: {IP}\n\n")),
        "{stdout}"
    );
    let verdict = |fqdn: &str| {
        stdout
            .lines()
            .find(|l| l.trim_start().starts_with(fqdn))
            .unwrap_or_else(|| panic!("no row for {fqdn}: {stdout}"))
            .to_string()
    };
    assert!(verdict("ok.example.com").contains("Up to date"));
    assert!(verdict("ttl.example.com").contains("Needs update"));
    assert!(verdict("gone.example.com").contains("Not found"));

    let mut cmd = env.cmd();
    cmd.args(["status", "--format", "json", "-c"]).arg(&cfg);
    let (stdout, _, code) = run(cmd).await;
    assert_eq!(code, Some(0));
    let v: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["ip"], IP);
    let statuses: Vec<&str> = v["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, vec!["up_to_date", "needs_update", "not_found"]);
    assert_eq!(v["targets"][1]["record"]["ttl"], 300);
}

#[tokio::test]
async fn list_records_and_domains() {
    let env = Env::new().await;
    env.records(
        "example.com",
        json!([
            record("r1", "", "45.77.1.2", 300),
            {"id": "ns", "type": "NS", "name": "", "data": "ns1.vultr.com", "priority": -1, "ttl": 300}
        ]),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/domains"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "domains": [{"domain": "example.com", "date_created": "2024-01-01T00:00:00+00:00", "dns_sec": "disabled"}],
            "meta": {"total": 1, "links": {"next": "", "prev": ""}}
        })))
        .mount(&env.vultr)
        .await;

    let mut cmd = env.cmd();
    cmd.env("VULTR_API_KEY", KEY)
        .args(["list-records", "example.com"]);
    let (stdout, stderr, code) = run(cmd).await;
    assert_eq!(code, Some(0), "{stderr}");
    let header: Vec<&str> = stdout.lines().next().unwrap().split_whitespace().collect();
    assert_eq!(header, ["ID", "Type", "Name", "Data", "TTL", "Priority"]);
    let ns: Vec<&str> = stdout.lines().nth(2).unwrap().split_whitespace().collect();
    assert_eq!(ns, ["ns", "NS", "@", "ns1.vultr.com", "300", "-1"]);

    let mut cmd = env.cmd();
    cmd.env("VULTR_API_KEY", KEY)
        .args(["list-records", "example.com", "--format", "json"]);
    let (stdout, _, _) = run(cmd).await;
    let v: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v[0]["name"], "");
    assert_eq!(v[1]["priority"], -1);

    let mut cmd = env.cmd();
    cmd.env("VULTR_API_KEY", KEY).args(["list-domains"]);
    let (stdout, stderr, code) = run(cmd).await;
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains("example.com"), "{stdout}");
    assert!(stdout.contains("2024-01-01T00:00:00+00:00"), "{stdout}");
}

#[tokio::test]
async fn init_config_then_show_example() {
    let env = Env::new().await;
    let mut cmd = env.cmd();
    cmd.arg("init-config");
    let (stdout, stderr, code) = run(cmd).await;
    assert_eq!(code, Some(0), "{stderr}");
    let path = env.home().join(".config/vultr-dns-updater/config.toml");
    assert_eq!(
        stdout,
        format!(
            "Success: Created example config at: {}\n\nEdit the file to add your Vultr API key and targets.\n",
            path.display()
        )
    );
    let mut cmd = env.cmd();
    cmd.arg("show-config-example");
    let (stdout, _, _) = run(cmd).await;
    assert_eq!(
        stdout,
        format!("{}\n", std::fs::read_to_string(&path).unwrap())
    );

    let mut cmd = env.cmd();
    cmd.arg("init-config");
    let (_, stderr, code) = run(cmd).await;
    assert_eq!(code, Some(1));
    assert!(
        stderr.starts_with("Error: Configuration file already exists: "),
        "{stderr}"
    );
}

#[tokio::test]
async fn self_update_refuses_on_a_dev_build() {
    let env = Env::new().await;
    let mut cmd = env.cmd();
    cmd.args(["self", "update", "--check"]);
    let (_, stderr, code) = run(cmd).await;
    assert_eq!(code, Some(1));
    assert!(stderr.contains("dev build (0.0.0)"), "{stderr}");
}
