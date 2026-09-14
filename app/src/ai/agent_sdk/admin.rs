//! General-purpose administrative commands in the Warp CLI.

use anyhow::{Context, Result};
use serde::Serialize;
use warp_cli::agent::OutputFormat;
use warpui::platform::TerminationMode;
use warpui::{AppContext, SingletonEntity};

use crate::auth::user::PrincipalType;
use crate::auth::{AuthStateProvider, UserUid};
use crate::workspaces::user_workspaces::UserWorkspaces;
use crate::workspaces::workspace::Workspace;

#[derive(Serialize)]
struct WhoamiOutput {
    uid: String,
    #[serde(rename = "type")]
    principal_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    team_uids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    team_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_name: Option<String>,
}

impl WhoamiOutput {
    fn set_workspace(&mut self, workspace: Option<&Workspace>, user_uid: UserUid) {
        let Some(workspace) = workspace else {
            return;
        };
        let teams: Vec<_> = workspace
            .teams
            .iter()
            .filter(|team| team.members.iter().any(|member| member.uid == user_uid))
            .collect();

        self.team_uids = teams.iter().map(|team| team.uid.to_string()).collect();
        self.team_names = teams.iter().map(|team| team.name.clone()).collect();
        self.workspace_uid = Some(workspace.uid.into());
        self.workspace_name = (!workspace.name.is_empty()).then(|| workspace.name.clone());
    }

    fn pretty(&self, principal_type: PrincipalType) -> String {
        let mut lines = vec![match principal_type {
            PrincipalType::User => format!("User ID: {}", self.uid),
            PrincipalType::ServiceAccount => format!("Service account ID: {}", self.uid),
        }];

        if let Some(name) = &self.display_name {
            lines.push(format!("Display Name: {name}"));
        }
        if let Some(email) = &self.email {
            lines.push(format!("Email: {email}"));
        }

        if let Some(workspace_uid) = &self.workspace_uid {
            lines.push(format!("Workspace UID: {workspace_uid}"));
        }
        if let Some(workspace_name) = &self.workspace_name {
            lines.push(format!("Workspace Name: {workspace_name}"));
        }
        if self.team_uids.len() > 1 {
            lines.push("Teams:".to_string());
        }

        for (team_uid, team_name) in self.team_uids.iter().zip(&self.team_names) {
            let indent = if self.team_uids.len() > 1 { "  " } else { "" };
            lines.push(format!("{indent}Team ID: {team_uid}"));
            if !team_name.is_empty() {
                lines.push(format!("{indent}Team Name: {team_name}"));
            }
        }

        lines.join("\n")
    }
}

/// Singleton model that provides a `ModelContext` for the `whoami` command's async work.
struct WhoamiRunner;

impl warpui::Entity for WhoamiRunner {
    type Event = ();
}

impl SingletonEntity for WhoamiRunner {}

/// Print information about the currently authenticated principal.
pub fn whoami(ctx: &mut AppContext, output_format: OutputFormat) -> Result<()> {
    let auth_state = AuthStateProvider::as_ref(ctx).get();
    let principal_type = auth_state.principal_type().unwrap_or_default();

    let user_uid = auth_state
        .user_id()
        .ok_or_else(|| anyhow::anyhow!("Could not determine user ID. Are you logged in?"))?;
    let uid = user_uid.as_string();
    let uid = uid
        .strip_prefix("serviceAccount:")
        .map(String::from)
        .unwrap_or(uid);

    let mut info = WhoamiOutput {
        uid,
        principal_type: match principal_type {
            PrincipalType::User => "user",
            PrincipalType::ServiceAccount => "service_account",
        },
        display_name: auth_state.display_name(),
        email: match principal_type {
            PrincipalType::User => auth_state.user_email().filter(|e| !e.is_empty()),
            PrincipalType::ServiceAccount => None,
        },
        team_uids: vec![],
        team_names: vec![],
        workspace_uid: None,
        workspace_name: None,
    };

    // Read team info from the persisted workspace state.
    let runner = ctx.add_singleton_model(|_| WhoamiRunner);
    runner.update(ctx, move |_, ctx| {
        info.set_workspace(UserWorkspaces::as_ref(ctx).current_workspace(), user_uid);

        match output_format {
            OutputFormat::Json => {
                match serde_json::to_string(&info).context("whoami output should serialize") {
                    Ok(json) => println!("{json}"),
                    Err(err) => {
                        ctx.terminate_app(TerminationMode::ForceTerminate, Some(Err(err)));
                        return;
                    }
                }
            }
            OutputFormat::Pretty => {
                println!("{}", info.pretty(principal_type));
            }
            OutputFormat::Text => {
                println!("{}:{}", info.principal_type, info.uid);
            }
            OutputFormat::Ndjson => {
                ctx.terminate_app(
                    TerminationMode::ForceTerminate,
                    Some(Err(anyhow::anyhow!(
                        "`whoami` does not support `--output-format ndjson`"
                    ))),
                );
                return;
            }
        }

        ctx.terminate_app(TerminationMode::ForceTerminate, None);
    });

    Ok(())
}

/// Log out of Warp using the same logic as the app.
pub fn logout(ctx: &mut AppContext) -> Result<()> {
    let auth_state = AuthStateProvider::as_ref(ctx).get();
    if !auth_state.is_logged_in() {
        println!("You are not logged in.");
        ctx.terminate_app(TerminationMode::ForceTerminate, None);
        return Ok(());
    }

    crate::auth::log_out(ctx);
    println!("Logged out successfully.");
    ctx.terminate_app(TerminationMode::ForceTerminate, None);
    Ok(())
}

#[cfg(test)]
#[path = "admin_tests.rs"]
mod tests;
