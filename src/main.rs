use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use structopt::StructOpt;
use tokio::task;

#[derive(Debug, StructOpt)]
#[structopt(name = "repos", about = "Github repo cloner")]
struct Opt {
    #[structopt(short, long)]
    clone: bool,

    #[structopt(short, long)]
    user: String,
}

#[derive(Deserialize, Debug)]
struct Repo {
    name: String,
    ssh_url: String,
    languages_url: String,

    #[serde(skip_deserializing)]
    lang: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    dotenv::dotenv().ok();

    let mut handles = vec![];

    for mut repo in get_repos(&opt.user).await? {
        let handle = task::spawn(async move {
            let lang = get_language(&repo.languages_url)
                .await
                .unwrap_or("other".to_string())
                .to_ascii_lowercase()
                .replace(" ", "_");
            repo.lang = lang;
            repo
        });
        handles.push(handle);
    }

    let mut clone_handles = vec![];

    for handle in handles {
        let repo = handle.await?;
        if opt.clone {
            let handle = task::spawn(clone_repo(repo));
            clone_handles.push(handle);
        }
    }

    for handle in clone_handles {
        let _ = handle.await??;
    }

    Ok(())
}

async fn clone_repo(repo: Repo) -> Result<()> {
    if !Path::new(&format!("{}/{}", repo.lang, repo.name)).is_dir() {
        if !Path::new(&repo.lang).is_dir() {
            fs::create_dir_all(&repo.lang)?;
        }
        let status = Command::new("git")
            .arg("-C")
            .arg(&repo.lang)
            .arg("clone")
            .arg(&repo.ssh_url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if status.success() {
            println!(
                "     clone OK: {}/{} ({})",
                &repo.lang, &repo.name, repo.ssh_url
            );
        } else {
            println!(
                " clone FAILED: {}/{} ({})",
                &repo.lang, &repo.name, repo.ssh_url
            );
        }
    } else {
        println!("skip existing: {}/{}", repo.lang, repo.name);
    }
    Ok(())
}

fn build_request(method: reqwest::Method, url: &str) -> Result<reqwest::RequestBuilder> {
    let client = reqwest::Client::new();
    let token = env::var("GITHUB_TOKEN").context("'GITHUB_TOKEN' environment variable missing")?;

    let rb = client
        .request(method, url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "rust")
        .header("Authorization", format!("token {}", token));
    Ok(rb)
}

async fn get_repos(user: &str) -> Result<Vec<Repo>> {
    println!("Getting repos {}:", user);
    let resp = build_request(
        reqwest::Method::GET,
        &format!("https://api.github.com/users/{}/repos", user),
    )?
    .send()
    .await?;

    if resp.status().is_success() {
        Ok(resp.json::<Vec<Repo>>().await?)
    } else {
        anyhow::bail!("{}", resp.text().await?)
    }
}

async fn get_language(url: &str) -> Result<String> {
    let resp = build_request(reqwest::Method::GET, url)?.send().await?;

    if resp.status().is_success() {
        let langs = resp.json::<HashMap<String, u32>>().await?;
        let (lang, _) = langs
            .iter()
            .max_by_key(|(_, v)| *v)
            .ok_or_else(|| anyhow!("languages not found"))?;
        Ok(lang.to_string())
    } else {
        anyhow::bail!("{}", resp.text().await?)
    }
}
