use crate::list::{ListKindOutput, ListOutput, Row};
use crate::source::{SourceBrowseResult, SourceListResult};

pub fn print_list_kind_output(output: &ListKindOutput) {
    let kind = output.kind;
    let global = &output.global_rows;
    let local = &output.local_rows;

    if global.is_empty() && local.is_empty() {
        println!("No {kind}s installed.");
        return;
    }

    if !global.is_empty() {
        println!("Global {kind}s:");
        print_table(global);
    }

    if !local.is_empty() {
        if !global.is_empty() {
            println!();
        }
        println!("Local {kind}s:");
        print_table(local);
    }
}

pub fn print_list_all_output(output: &ListOutput) {
    if output.global_agents.is_empty()
        && output.local_agents.is_empty()
        && output.global_skills.is_empty()
        && output.local_skills.is_empty()
    {
        println!("Nothing installed.");
        return;
    }

    print_section("Global agents", &output.global_agents);
    print_section("Local agents", &output.local_agents);
    print_section("Global skills", &output.global_skills);
    print_section("Local skills", &output.local_skills);
}

pub fn print_table(rows: &[Row]) {
    if rows.is_empty() {
        return;
    }

    let w_name = rows.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let w_inst = rows.iter().map(|r| r.installed.len()).max().unwrap_or(9).max(9);
    let w_src = rows.iter().map(|r| r.source.len()).max().unwrap_or(6).max(6);
    let w_avail = rows.iter().map(|r| r.available.len()).max().unwrap_or(9).max(9);

    println!(
        "  {:<w_name$}  {:<w_inst$}  {:<w_src$}  {:<w_avail$}",
        "Name", "Installed", "Source", "Available",
    );
    println!(
        "  {:<w_name$}  {:<w_inst$}  {:<w_src$}  {:<w_avail$}",
        "-".repeat(w_name),
        "-".repeat(w_inst),
        "-".repeat(w_src),
        "-".repeat(w_avail),
    );

    for row in rows {
        println!(
            "  {:<w_name$}  {:<w_inst$}  {:<w_src$}  {:<w_avail$}  {}",
            row.name, row.installed, row.source, row.available, row.status,
        );
    }
}

pub fn print_section(label: &str, rows: &[Row]) {
    println!("{label}:");
    if rows.is_empty() {
        println!("  (none)");
    } else {
        print_table(rows);
    }
    println!();
}

pub fn print_source_list(result: &SourceListResult) {
    if result.entries.is_empty() {
        println!("No sources registered.");
        println!();
        println!("Add one with: cmx source add <name> <path-or-url>");
        return;
    }

    for entry in &result.entries {
        println!("  {:<28} ({}) {}", entry.name, entry.kind, entry.location);
    }
}

pub fn print_browse_result(result: &SourceBrowseResult) {
    let name = &result.source_name;

    if result.agents.is_empty() && result.skills.is_empty() {
        println!("No agents or skills found in '{name}'.");
        return;
    }

    if !result.agents.is_empty() {
        println!("Agents:");
        for a in &result.agents {
            let v = a.version.as_deref().map(|v| format!("  v{v}")).unwrap_or_default();
            println!("  {}{v}{}", a.name, a.deprecation_display);
        }
    }

    if !result.skills.is_empty() {
        if !result.agents.is_empty() {
            println!();
        }
        println!("Skills:");
        for s in &result.skills {
            let v = s.version.as_deref().map(|v| format!("  v{v}")).unwrap_or_default();
            println!("  {}{v}{}", s.name, s.deprecation_display);
            for f in &s.files {
                println!("    {f}");
            }
        }
    }
}
