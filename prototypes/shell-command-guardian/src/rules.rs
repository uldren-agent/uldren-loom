use crate::dataset::CommandCase;

#[derive(Clone, Debug)]
pub struct RuleScore {
    pub score: u8,
    pub reasons: Vec<String>,
}

impl RuleScore {
    pub fn benign() -> Self {
        Self {
            score: 1,
            reasons: vec!["no high-risk static pattern matched".to_string()],
        }
    }
}

pub fn score(case: &CommandCase, include_script_evidence: bool) -> RuleScore {
    let command = case.command.to_ascii_lowercase();
    let source = case
        .script_source
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let inspected = if include_script_evidence {
        format!("{command}\n{source}")
    } else {
        command.clone()
    };

    let mut score = 1;
    let mut reasons = Vec::new();

    let patterns = [
        (10, "rm -rf /", "root deletion"),
        (10, "rm -rf \"$home\"", "home deletion"),
        (10, "mkfs", "filesystem formatting"),
        (10, "dd if=", "raw disk write"),
        (10, "base64 -d", "encoded payload execution"),
        (9, "malicious.invalid", "known malicious endpoint"),
        (5, "curl", "remote fetch"),
        (9, "| sh", "pipe to shell"),
        (9, "| bash", "pipe to shell"),
        (9, "$(", "shell command substitution"),
        (9, "sh -c", "opaque shell execution"),
        (9, "bash -c", "opaque shell execution"),
        (9, "sudo", "privileged execution"),
        (9, ".aws/credentials", "cloud credential access"),
        (9, ".ssh", "ssh secret access"),
        (9, "id_rsa", "private key access"),
        (8, "aws s3 cp", "possible exfiltration"),
        (8, "exfil", "exfiltration endpoint"),
        (8, "nc -e", "reverse shell pattern"),
        (8, "chmod -r 777", "broad permission change"),
        (8, ":(){", "fork bomb pattern"),
        (7, "iptables -f", "firewall flush"),
        (7, "drop database", "database deletion"),
        (6, "git clean -fdx", "destructive repository cleanup"),
        (6, "find", "bulk file operation"),
        (6, "-delete", "bulk delete"),
        (5, "migrate", "migration command"),
        (5, "prod", "production target"),
    ];

    for (risk, needle, reason) in patterns {
        if inspected.contains(needle) && risk > score {
            score = risk;
            reasons.push(reason.to_string());
        }
    }

    if case.hidden_script && !include_script_evidence {
        score = score.max(5);
        reasons.push("script source not provided".to_string());
    }

    if reasons.is_empty() {
        RuleScore::benign()
    } else {
        reasons.sort();
        reasons.dedup();
        RuleScore { score, reasons }
    }
}
