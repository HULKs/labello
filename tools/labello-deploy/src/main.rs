use std::{env, io, path::PathBuf, process::ExitCode};

use anyhow::{Context, Result, bail};
use labello_deploy::{
    DeploymentManager, RealPlatform, ReceiveOptions, create_release_manifest, verify_release_assets,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(_error) => {
            eprintln!("labello deployment failed; inspect the local deployment status");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut arguments = env::args().skip(1);
    let command = arguments.next().context("missing command")?;
    let root = env::var_os("LABELLO_DEPLOY_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/labello"));

    match command.as_str() {
        "manifest" => {
            let candidate_root = arguments.next().context("missing candidate root")?;
            let release_tag = arguments.next().context("missing release tag")?;
            let source_commit = arguments.next().context("missing source commit")?;
            no_more_arguments(&mut arguments)?;
            create_release_manifest(PathBuf::from(candidate_root), &release_tag, &source_commit)?;
        }
        "verify-release" => {
            let assets_root = arguments.next().context("missing release assets root")?;
            let release_tag = arguments.next().context("missing release tag")?;
            let source_commit = arguments.next().context("missing source commit")?;
            no_more_arguments(&mut arguments)?;
            verify_release_assets(PathBuf::from(assets_root), &release_tag, &source_commit)?;
        }
        "receive" => {
            let manager = DeploymentManager::new(root, RealPlatform::from_environment()?);
            let request_id = one_argument(&mut arguments, "request ID")?;
            manager.receive(
                &request_id,
                io::stdin().lock(),
                ReceiveOptions { start_worker: true },
            )?;
        }
        "worker" => {
            let manager = DeploymentManager::new(root, RealPlatform::from_environment()?);
            let request_id = one_argument(&mut arguments, "request ID")?;
            manager.run(&request_id)?;
        }
        "recover" => {
            let manager = DeploymentManager::new(root, RealPlatform::from_environment()?);
            no_more_arguments(&mut arguments)?;
            manager.boot_recover()?;
        }
        "status" => {
            let manager = DeploymentManager::new(root, RealPlatform::from_environment()?);
            let request_id = one_argument(&mut arguments, "request ID")?;
            let status = manager.status(&request_id)?;
            println!("{}", serde_json::to_string(&status)?);
        }
        _ => bail!("unsupported command"),
    }
    Ok(())
}

fn one_argument(arguments: &mut impl Iterator<Item = String>, name: &str) -> Result<String> {
    let value = arguments
        .next()
        .with_context(|| format!("missing {name}"))?;
    no_more_arguments(arguments)?;
    Ok(value)
}

fn no_more_arguments(arguments: &mut impl Iterator<Item = String>) -> Result<()> {
    if arguments.next().is_some() {
        bail!("unexpected argument");
    }
    Ok(())
}
