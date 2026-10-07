/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use clap::Parser;

use vandelay::cli::{Action, Cli};
use vandelay::db::canonical_email;
use vandelay::error::Error;
use vandelay::inspect;
use vandelay::sync::{self, RunOutcome, Summary};

fn main() {
    let code = run();
    std::process::exit(code);
}

fn run() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let _ = err.print();
            return if err.use_stderr() { 1 } else { 0 };
        }
    };

    let action = match cli.resolve() {
        Ok(action) => action,
        Err(err) => return fail(&err),
    };

    let (outcome, logger) = match action {
        Action::ConsolidateGmail { archive, apply } => {
            return run_gmail_consolidation(&archive, apply);
        }
        Action::Import(common, config) => {
            let logger = common.logger;
            (sync::import_jmap::run_reporting(common, config), logger)
        }
        Action::ImportImap(common, config) => {
            let logger = common.logger;
            (sync::import_imap::run_reporting(common, config), logger)
        }
        Action::ImportDav(common, config) => {
            let logger = common.logger;
            (sync::import_dav::run_reporting(common, config), logger)
        }
        Action::ImportManageSieve(common, config) => {
            let logger = common.logger;
            (
                RunOutcome::from_result(sync::import_managesieve::run(common, config)),
                logger,
            )
        }
        Action::ImportMaildir(common, config) => {
            let logger = common.logger;
            (sync::import_maildir::run_reporting(common, config), logger)
        }
        Action::ImportTakeout(common, config) => {
            let logger = common.logger;
            (sync::import_takeout::run_reporting(common, config), logger)
        }
        Action::ImportExchangeEws(common, config) => {
            let logger = common.logger;
            (
                RunOutcome::from_result(sync::import_exchange_ews::run(common, config)),
                logger,
            )
        }
        Action::ImportExchangeGraph(common, config) => {
            let logger = common.logger;
            (
                RunOutcome::from_result(sync::import_exchange_graph::run(common, config)),
                logger,
            )
        }
        Action::Export(common, config) => {
            let logger = common.logger;
            (
                RunOutcome::from_result(sync::export::run(common, config)),
                logger,
            )
        }
        Action::Inspect(config) => {
            return match inspect::run(config) {
                Ok(()) => 0,
                Err(err) => fail(&err),
            };
        }
    };

    report(&outcome.summary);
    match outcome.error {
        Some(err) => fail(&err),
        None => {
            if outcome.summary.any_failed() {
                logger.error("some objects failed; the archive is consistent and resumable");
                5
            } else {
                0
            }
        }
    }
}

fn run_gmail_consolidation(archive: &std::path::Path, apply: bool) -> i32 {
    let mut conn = match rusqlite::Connection::open(archive) {
        Ok(conn) => conn,
        Err(err) => return fail(&Error::from(err)),
    };
    let analysis = match canonical_email::analyze_gmail_identities(&conn) {
        Ok(analysis) => analysis,
        Err(err) => return fail(&Error::from(err)),
    };

    println!("Gmail identity consolidation analysis:");
    println!("  observations={}", analysis.observations);
    println!("  populated={}", analysis.populated);
    println!("  missing={}", analysis.missing);
    println!("  unique_x_gm_msgid={}", analysis.gmail_identities);
    println!(
        "  duplicate_identity_groups={}",
        analysis.duplicate_identity_groups
    );
    println!("  rows_removed={}", analysis.rows_removed);
    println!("  blob_conflict_groups={}", analysis.blob_conflict_groups);
    println!(
        "  keyword_conflict_groups={}",
        analysis.keyword_conflict_groups
    );
    println!(
        "  multi_identity_local_rows={}",
        analysis.multi_identity_local_rows
    );
    println!("  max_rows_per_identity={}", analysis.max_rows_per_identity);
    println!("  email_rows_before={}", analysis.email_rows_before);
    println!("  email_rows_after={}", analysis.email_rows_after);
    println!("  safe_to_apply={}", analysis.safe_to_apply());

    if !apply {
        println!(
            "read-only analysis; rerun with --apply only after reviewing this report and taking a checkpoint"
        );
        return 0;
    }
    if !analysis.safe_to_apply() {
        return fail(&Error::Usage(
            "Gmail consolidation is blocked: missing identities, conflicting blobs/keywords, or a local Email spans multiple Gmail identities"
                .to_owned(),
        ));
    }

    let result = match canonical_email::consolidate_by_gmail_identity(&mut conn) {
        Ok(result) => result,
        Err(err) => return fail(&Error::from(err)),
    };
    println!("Gmail identity consolidation applied:");
    println!("  groups={}", result.groups);
    println!("  removed_rows={}", result.removed_rows);
    println!(
        "  remapped_imap_observations={}",
        result.remapped_imap_observations
    );
    println!(
        "  removed_export_mappings={}",
        result.removed_export_mappings
    );
    println!(
        "  removed_takeout_mappings={}",
        result.removed_takeout_mappings
    );
    println!(
        "  remapped_takeout_mappings={}",
        result.remapped_takeout_mappings
    );
    0
}

fn fail(err: &Error) -> i32 {
    eprintln!("error: {err}");
    err.exit_code()
}

fn report(summary: &Summary) {
    for (type_name, counts) in &summary.per_type {
        println!(
            "{type_name}: created={} fetched={} updated={} deleted={} skipped={} failed={}",
            counts.created,
            counts.fetched,
            counts.updated,
            counts.deleted,
            counts.skipped,
            counts.failed
        );
    }
}
