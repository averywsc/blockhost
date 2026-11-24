//! Plain-English crash analyzer: inspects recent server logs when Paper or Fabric exits
//! abnormally and explains the likely cause + next step in non-technical language.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrashDiagnosis {
    pub title: String,
    pub explanation: String,
    pub suggestion: String,
}

/// Analyze recent console lines and return a plain-English diagnosis if a known failure pattern matches.
pub fn analyze_logs(lines: &[String]) -> Option<CrashDiagnosis> {
    let joined = lines
        .iter()
        .rev()
        .take(120)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let lower = joined.to_ascii_lowercase();

    if lower.contains("outofmemoryerror") || lower.contains("java heap space") {
        return Some(CrashDiagnosis {
            title: "Server ran out of memory".into(),
            explanation: "Minecraft needed more RAM than its current limit allows.".into(),
            suggestion: "Increase Memory for Minecraft in Settings, or turn on Low-lag mode in World.".into(),
        });
    }

    if lower.contains("failed to bind to port") || lower.contains("address already in use") {
        return Some(CrashDiagnosis {
            title: "Port is already in use".into(),
            explanation: "Another Minecraft server or program is already using this server's port.".into(),
            suggestion: "Close any other Minecraft server window, or restart your PC and try again.".into(),
        });
    }

    if lower.contains("unsupportedclassversionerror")
        || lower.contains("compiled by a more recent version of the java runtime")
    {
        return Some(CrashDiagnosis {
            title: "Java version mismatch".into(),
            explanation: "This Minecraft version or one of its mods needs a newer Java runtime.".into(),
            suggestion: "Check that your mods match this Minecraft version, or recreate the server with a matching version.".into(),
        });
    }

    if lower.contains("incompatible mods found")
        || lower.contains("mod resolution failed")
        || lower.contains("missing or unsupported mandatory dependencies")
        || lower.contains("could not load 'plugins")
    {
        return Some(CrashDiagnosis {
            title: "Mod or plugin conflict".into(),
            explanation: "One of your installed mods or plugins is incompatible with this Minecraft version or is missing a required dependency (like Fabric API).".into(),
            suggestion: "Open the Mods/Plugins tab, check if a dependency is missing, or remove the most recently added file.".into(),
        });
    }

    if lower.contains("failed to check session lock") || lower.contains("session.lock") {
        return Some(CrashDiagnosis {
            title: "World folder is locked".into(),
            explanation: "Another process is still holding the world lock file.".into(),
            suggestion: "Make sure no old Java server is still running in Task Manager, then start again."
                .into(),
        });
    }

    if lower.contains("you need to agree to the eula") {
        return Some(CrashDiagnosis {
            title: "Minecraft EULA not accepted".into(),
            explanation: "The server requires eula=true in eula.txt before it can start.".into(),
            suggestion: "Accept the EULA in the setup wizard and try again.".into(),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnoses_oom_port_and_mod_conflicts() {
        let oom = analyze_logs(&["java.lang.OutOfMemoryError: Java heap space".into()]).unwrap();
        assert_eq!(oom.title, "Server ran out of memory");

        let port = analyze_logs(&["[WARN] **** FAILED TO BIND TO PORT!".into()]).unwrap();
        assert_eq!(port.title, "Port is already in use");

        let mods = analyze_logs(&["Incompatible mods found! net.fabricmc.loader".into()]).unwrap();
        assert_eq!(mods.title, "Mod or plugin conflict");

        assert!(analyze_logs(&["[INFO] Done (4.2s)! For help, type help".into()]).is_none());
    }
}
