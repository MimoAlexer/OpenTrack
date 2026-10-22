//! One explicit credential source per request; browser and native audiences never mix.
use crate::{ApiError, ApiState};
use axum::http::{HeaderMap, header};
use opentrack_domain::{Actor, Error};

pub(crate) enum Credential<'a> {
    None,
    Web(&'a str),
    Desktop(&'a str),
}

pub(crate) fn extract(headers: &HeaderMap) -> Result<Credential<'_>, ApiError> {
    let mut cookie = None;
    for value in headers.get_all(header::COOKIE) {
        let value = value.to_str().map_err(|_| Error::Unauthorized)?;
        for part in value.split(';').map(str::trim) {
            let (name, value) = part.split_once('=').unwrap_or((part, ""));
            if name.trim() == "opentrack_session" {
                if cookie.is_some() || value.is_empty() {
                    return Err(Error::Unauthorized.into());
                }
                cookie = Some(value);
            }
        }
    }
    let mut authorization = headers.get_all(header::AUTHORIZATION).iter();
    let bearer = if let Some(value) = authorization.next() {
        if authorization.next().is_some() || cookie.is_some() {
            return Err(Error::Unauthorized.into());
        }
        let value = value.to_str().map_err(|_| Error::Unauthorized)?;
        let mut parts = value.split_ascii_whitespace();
        let scheme = parts.next().ok_or(Error::Unauthorized)?;
        let token = parts.next().ok_or(Error::Unauthorized)?;
        if !scheme.eq_ignore_ascii_case("Bearer") || parts.next().is_some() {
            return Err(Error::Unauthorized.into());
        }
        Some(token)
    } else {
        None
    };
    Ok(match (cookie, bearer) {
        (Some(token), None) => Credential::Web(token),
        (None, Some(token)) => Credential::Desktop(token),
        (None, None) => Credential::None,
        _ => return Err(Error::Unauthorized.into()),
    })
}

pub(crate) fn browser_cookie(headers: &HeaderMap) -> Result<Option<&str>, ApiError> {
    match extract(headers)? {
        Credential::None => Ok(None),
        Credential::Web(token) => Ok(Some(token)),
        Credential::Desktop(_) => Err(Error::Unauthorized.into()),
    }
}

pub(crate) async fn authenticate(state: &ApiState, headers: &HeaderMap) -> Result<Actor, ApiError> {
    Ok(match extract(headers)? {
        Credential::Web(token) => state.app.authenticate(token).await?,
        Credential::Desktop(token) => state.app.authenticate_native(token).await?,
        Credential::None => return Err(Error::Unauthorized.into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn rejects_ambiguous_or_malformed_credentials() {
        for values in [
            vec![(
                "cookie",
                "opentrack_session=first; opentrack_session=second",
            )],
            vec![
                ("cookie", "opentrack_session=first"),
                ("cookie", "opentrack_session=first"),
            ],
            vec![
                ("cookie", "opentrack_session=first"),
                ("authorization", "Bearer second"),
            ],
            vec![
                ("authorization", "Bearer first"),
                ("authorization", "Bearer second"),
            ],
            vec![("authorization", "Bearer first,second extra")],
            vec![("authorization", "Basic first")],
            vec![("authorization", "Bearer")],
            vec![("cookie", "opentrack_session=")],
            vec![("cookie", "opentrack_session")],
        ] {
            let mut headers = HeaderMap::new();
            for (name, value) in values {
                headers.append(name, HeaderValue::from_static(value));
            }
            assert!(extract(&headers).is_err());
        }
    }

    #[test]
    fn browser_routes_reject_bearer_even_with_unrelated_cookies() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_static("theme=dark"));
        assert!(matches!(extract(&headers).unwrap(), Credential::None));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer native"),
        );
        assert!(matches!(
            extract(&headers).unwrap(),
            Credential::Desktop("native")
        ));
        assert!(browser_cookie(&headers).is_err());
        headers.remove(header::AUTHORIZATION);
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("opentrack_session=web"),
        );
        assert_eq!(browser_cookie(&headers).unwrap(), Some("web"));
    }
}
