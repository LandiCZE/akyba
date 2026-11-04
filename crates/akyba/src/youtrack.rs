//! The YouTrack connection: your YouTrack's address (YouTrack Cloud, or a server of your own) and
//! a permanent token, kept together in a file only you can read, and the few calls Akyba makes
//! with them. Everything here blocks, so callers run it off the main thread. Without a token,
//! nothing about YouTrack shows anywhere.

use akyba_core::tickets::Ticket;
use akyba_core::youtrack;
use serde::{Deserialize, Serialize};

use crate::private_file;

const LOGIN_FILE: &str = "youtrack.json";
/// More than this many pages of open issues assigned to one person is not a list anyone picks from.
const MAX_PAGES: usize = 5;

/// Where YouTrack is and the token to reach it with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Login {
    /// The instance's address, as `youtrack::instance_url` gives it: `https://youtrack.acme.dev`.
    pub url: String,
    pub token: String,
}

/// The saved login, if one was set.
pub fn login() -> Option<Login> {
    let login: Login = serde_json::from_str(&private_file::read(LOGIN_FILE)?).ok()?;
    (!login.url.is_empty() && !login.token.is_empty()).then_some(login)
}

/// Saves the login where only your user can read it (permissions 0600).
pub fn save_login(login: &Login) -> Result<(), String> {
    let json = serde_json::to_string(login).map_err(|error| error.to_string())?;
    private_file::write(LOGIN_FILE, &json)
}

pub fn forget_login() {
    private_file::remove(LOGIN_FILE);
}

/// The host, to say which YouTrack an error is about.
fn host(url: &str) -> &str {
    url.split_once("://").map_or(url, |(_, rest)| rest)
}

/// `GET {url}/api{path}` with the token and `query`; an explanation on failure.
fn get(agent: &ureq::Agent, login: &Login, path: &str, query: &[(&str, &str)]) -> Result<String, String> {
    let at = host(&login.url);
    let mut request = agent
        .get(&format!("{}/api{path}", login.url))
        .header("Authorization", &format!("Bearer {}", login.token))
        .header("Accept", "application/json");
    for (key, value) in query {
        request = request.query(*key, *value);
    }
    let mut response = request.call().map_err(|error| format!("Could not reach YouTrack at {at}: {error}"))?;
    let status = response.status().as_u16();
    let body =
        response.body_mut().read_to_string().map_err(|error| format!("YouTrack's answer was cut off: {error}"))?;
    match status {
        200..=299 => Ok(body),
        401 => Err("YouTrack did not accept the token.".to_owned()),
        403 => Err("The token may not read issues in YouTrack.".to_owned()),
        404 => Err(format!("There's no YouTrack at {at}. Check the address.")),
        429 => Err("YouTrack asks to slow down; trying again in a minute.".to_owned()),
        _ => Err(format!("YouTrack at {at} answered {status}.")),
    }
}

/// An answer that isn't YouTrack's JSON is most often a sign-in page or another site.
fn unexpected(login: &Login) -> String {
    format!("{} answered, but not as YouTrack does. Check the address.", host(&login.url))
}

/// Who the token belongs to, to check a login when it's typed: "Jane Doe at youtrack.acme.dev".
pub fn check(login: &Login) -> Result<String, String> {
    let json = get(&crate::http::agent(), login, "/users/me", &[("fields", "login,fullName")])?;
    let name = youtrack::parse_me(&json).map_err(|_| unexpected(login))?;
    Ok(format!("{name} at {}", host(&login.url)))
}

/// The open issues assigned to the token's owner.
pub fn my_tickets(login: &Login) -> Result<Vec<Ticket>, String> {
    let agent = crate::http::agent();
    let mut tickets = Vec::new();
    let top = youtrack::PAGE_SIZE.to_string();
    for page in 0..MAX_PAGES {
        let skip = (page * youtrack::PAGE_SIZE).to_string();
        let query = [
            ("query", youtrack::MY_OPEN_ISSUES),
            ("fields", youtrack::ISSUE_FIELDS),
            ("$top", top.as_str()),
            ("$skip", skip.as_str()),
        ];
        let json = get(&agent, login, "/issues", &query)?;
        let (mut found, more) = youtrack::parse_issue_page(&json, &login.url).map_err(|_| unexpected(login))?;
        tickets.append(&mut found);
        if !more {
            break;
        }
    }
    Ok(tickets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_name_the_host() {
        assert_eq!(host("https://youtrack.acme.dev/youtrack"), "youtrack.acme.dev/youtrack");
    }

    /// Reaches YouTrack Cloud's own instance with a token it can't accept: the connection, the
    /// endpoint and the error message, without an account. Needs the network, so it runs on request.
    #[test]
    #[ignore = "talks to youtrack.jetbrains.com"]
    fn a_wrong_token_is_turned_away_politely() {
        let login =
            Login { url: "https://youtrack.jetbrains.com".to_owned(), token: "perm:not-a-real-token".to_owned() };
        assert_eq!(check(&login), Err("YouTrack did not accept the token.".to_owned()));
    }
}
