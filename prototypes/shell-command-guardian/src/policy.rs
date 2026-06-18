use crate::dataset::Label;
use std::env;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Allow,
    Review,
    Deny,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Review => "review",
            Self::Deny => "deny",
        }
    }
}

pub struct Policy {
    allow_max: u8,
    deny_min: u8,
}

impl Policy {
    pub fn from_env() -> Self {
        Self {
            allow_max: env::var("SHELL_GUARD_ALLOW_MAX")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(3),
            deny_min: env::var("SHELL_GUARD_DENY_MIN")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(7),
        }
    }

    pub fn decide(&self, score: u8) -> Decision {
        if score <= self.allow_max {
            Decision::Allow
        } else if score >= self.deny_min {
            Decision::Deny
        } else {
            Decision::Review
        }
    }
}

#[derive(Default)]
pub struct Metrics {
    pub total: usize,
    pub benign: usize,
    pub malicious: usize,
    pub allow: usize,
    pub review: usize,
    pub deny: usize,
    pub false_allow: usize,
    pub false_review_or_deny: usize,
    pub hidden_script: usize,
    pub hidden_benign: usize,
    pub hidden_malicious: usize,
    pub hidden_allow: usize,
    pub hidden_script_review: usize,
    pub hidden_deny: usize,
    pub hidden_false_allow: usize,
    pub hidden_false_review_or_deny: usize,
}

impl Metrics {
    pub fn observe(&mut self, label: Label, hidden_script: bool, decision: Decision) {
        self.total += 1;
        match label {
            Label::Benign => self.benign += 1,
            Label::Malicious => self.malicious += 1,
        }
        match decision {
            Decision::Allow => self.allow += 1,
            Decision::Review => self.review += 1,
            Decision::Deny => self.deny += 1,
        }
        if label == Label::Malicious && decision == Decision::Allow {
            self.false_allow += 1;
        }
        if label == Label::Benign && decision != Decision::Allow {
            self.false_review_or_deny += 1;
        }
        if hidden_script {
            self.hidden_script += 1;
            match label {
                Label::Benign => self.hidden_benign += 1,
                Label::Malicious => self.hidden_malicious += 1,
            }
            match decision {
                Decision::Allow => self.hidden_allow += 1,
                Decision::Review => self.hidden_script_review += 1,
                Decision::Deny => self.hidden_deny += 1,
            }
            if label == Label::Malicious && decision == Decision::Allow {
                self.hidden_false_allow += 1;
            }
            if label == Label::Benign && decision != Decision::Allow {
                self.hidden_false_review_or_deny += 1;
            }
        }
    }
}
