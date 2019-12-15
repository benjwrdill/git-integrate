extern crate git2;
extern crate graphql_client;
extern crate reqwest;
extern crate serde;
extern crate serde_derive;

mod git_extras;

use git2::{Config, Repository, Status};
use git_extras::Repo;
use graphql_client::{GraphQLQuery, Response};
use std::process::{Command, ExitStatus};
use std::{env, io, process};

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/github/schema.json",
    query_path = "src/github/queries.graphql",
    response_derives = "Debug,Clone"
)]
pub struct LabelBranches;

fn main() {
    let mut args = env::args().skip(1);

    let label = match args.next() {
        Some(label) => label,
        None => panic!("No github label provided"),
    };

    let dest_branch = match args.next() {
        Some(dest_branch) => dest_branch,
        None => panic!("No branch provided"),
    };

    let current_dir = match env::current_dir() {
        Ok(current_dir) => current_dir,
        Err(e) => panic!("{}", e),
    };

    let repository = match Repository::discover(current_dir.as_path()) {
        Ok(repository) => repository,
        Err(e) => panic!("{}", e),
    };

    let remote = match repository.find_remote("origin") {
        Ok(remote) => remote,
        Err(e) => panic!("{}", e),
    };

    let repo = match Repo::new(&remote) {
        Some(repo) => repo,
        None => panic!("Could not build remote info"),
    };

    let config = Config::open_default().expect("Could not find a git configuration file!");
    let github_token = config
        .get_string("integrate.github-token")
        .expect("Could not find integrate.github-token in any git configuration file!");

    if !git_fetch().expect("Error fetching from remote").success() {
        process::exit(1)
    }

    if !git_checkout(&dest_branch)
        .expect(&format!("Could not checkout branch {}", dest_branch))
        .success()
    {
        process::exit(1)
    }

    let branches = match branches(github_token, repo, label) {
        Ok(branches) => branches,
        Err(e) => panic!("{}", e),
    };

    for branch in branches {
        println!("\nMerging {}", branch);
        merge_branch(branch, &repository);
    }
}

fn branches(token: String, repo: Repo, label: String) -> Result<Vec<String>, reqwest::Error> {
    let q = LabelBranches::build_query(label_branches::Variables {
        owner: repo.owner,
        name: repo.name,
        label: label,
    });

    let client = reqwest::Client::new();

    let mut res = client
        .post("https://api.github.com/graphql")
        .bearer_auth(token)
        .json(&q)
        .send()?;

    let response: Response<label_branches::ResponseData> = res.json()?;

    Ok(response
        .data
        .and_then(|x| x.repository)
        .and_then(|x| x.pull_requests.nodes)
        .unwrap_or(vec![])
        .iter()
        .cloned()
        .filter_map(|x| x.map(|y| y.head_ref_name))
        .collect())
}

fn merge_branch(branch: String, repository: &Repository) {
    if !git_merge(&branch)
        .expect(&format!("Failure merging branch {}", branch))
        .success()
    {
        let dirty = repository
            .statuses(None)
            .expect("Error checking dirty repository")
            .iter()
            .any(|s| s.status() == Status::CONFLICTED);

        if dirty {
            println!(
                "\nMerge conflict detected, either fix the conflict and \
                 \nuse `git commit --no-edit` commit this merge or use \
                 \n`git merge --abort` to quit this merge"
            );
            process::exit(1);
        }

        if !git_commit()
            .expect(&format!("Failure merging branch {}", branch))
            .success()
        {
            println!("Failure mergeing branch {}", branch);
            process::exit(1);
        }
    }
}

fn git_fetch() -> io::Result<ExitStatus> {
    Command::new("git").arg("fetch").arg("--all").status()
}

fn git_checkout(branch: &String) -> io::Result<ExitStatus> {
    Command::new("git")
        .arg("checkout")
        .arg("--no-track")
        .arg("-B")
        .arg(branch)
        .arg("origin/master")
        .status()
}

fn git_merge(branch: &String) -> io::Result<ExitStatus> {
    Command::new("git")
        .arg("merge")
        .arg("--no-ff")
        .arg("--no-edit")
        .arg("--rerere-autoupdate")
        .arg("--log")
        .arg(&format!("origin/{}", branch))
        .status()
}

fn git_commit() -> io::Result<ExitStatus> {
    Command::new("git").arg("commit").arg("--no-edit").status()
}