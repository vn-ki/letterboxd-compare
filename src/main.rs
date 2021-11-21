#![feature(async_closure)]
#[macro_use]
extern crate lazy_static;

mod cache;
mod letterboxd;

use crate::cache::*;
use crate::letterboxd::*;
use anyhow::Result;
use askama::Template;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use tokio::try_join;
use tracing::{debug, info};
use tracing_subscriber;
use warp::Filter;

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    error_mess: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "diff.html")]
struct DiffTemplate<'a> {
    user1: &'a str,
    user2: &'a str,
    diff: Vec<Film>,
}

#[derive(Template)]
#[template(path = "and.html")]
struct AndTemplate<'a> {
    user1: &'a str,
    user2: &'a str,
    diff: Vec<(Film, Option<Rating>)>,
}

lazy_static! {
    static ref CLIENT: LetterboxdClient = LetterboxdClient::new();
    static ref CACHE: FileCache = FileCache::new().unwrap();
}

async fn cached_get_movies(username: &str) -> Result<Vec<Film>> {
    if let Some(s) = CACHE.get(username)? {
        info!("cache hit for {}", username);
        let ret: Vec<Film> = serde_json::from_str(&s)?;
        return Ok(ret);
    }
    let movies = CLIENT.get_movies_of_user(username).await?;
    CACHE.insert(username, &serde_json::to_string(&movies)?)?;
    Ok(movies)
}

async fn get_diff(user1: &str, user2: &str) -> Result<String> {
    info!("get_diff({}, {})", user1, user2);
    let (movies1, movies2) = try_join!(cached_get_movies(user1), cached_get_movies(user2))?;

    let watched_by_2: HashSet<_> = movies2.into_iter().map(|x| x.id).collect();

    let mut diff: Vec<_> = movies1
        .into_iter()
        .filter(|x| !watched_by_2.contains(&x.id))
        .collect();
    diff.sort_by(|a, b| a.rating.partial_cmp(&b.rating).unwrap().reverse());

    Ok(DiffTemplate {
        user1: &user1,
        user2: &user2,
        diff,
    }
    .render()
    .unwrap()
    .into())
}

async fn get_and(user1: &str, user2: &str) -> Result<String> {
    info!("get_and({}, {})", user1, user2);
    let (movies1, movies2) = try_join!(cached_get_movies(user1), cached_get_movies(user2))?;

    let watched_by_2: HashSet<_> = movies2.into_iter().collect();

    let mut diff: Vec<_> = movies1
        .into_iter()
        .filter_map(|film| watched_by_2.get(&film).map(|film_user2| (film, film_user2.rating)))
        .collect();
    diff.sort_by(|a, b| match a.0.rating.cmp(&b.0.rating).reverse() {
        // If the rating of movie i and i+1 are equal for user 1, then
        // sort by the rating of user 2.
        Ordering::Equal => a.1.cmp(&b.1).reverse(),
        other => other,
    });

    Ok(AndTemplate {
        user1: &user1,
        user2: &user2,
        diff,
    }
    .render()
    .unwrap()
    .into())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let port = std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or_else(|| 3030);

    let versus = warp::path!(String / "vs" / String).and_then(
        async move |user1: String,
                    user2: String|
                    -> Result<warp::reply::Html<String>, warp::reject::Rejection> {
            match get_diff(&user1, &user2).await {
                Ok(s) => Ok(warp::reply::html(s)),
                Err(err) => {
                    debug!("{:?}", &err);
                    Ok(warp::reply::html(
                        IndexTemplate {
                            error_mess: Some(&err.to_string()),
                        }
                        .render()
                        .unwrap()
                        .into(),
                    ))
                }
            }
        },
    );
    let same = warp::path!(String / "and" / String).and_then(
        async move |user1: String,
                    user2: String|
                    -> Result<warp::reply::Html<String>, warp::reject::Rejection> {
            match get_and(&user1, &user2).await {
                Ok(s) => Ok(warp::reply::html(s)),
                Err(err) => {
                    debug!("{:?}", &err);
                    Ok(warp::reply::html(
                        IndexTemplate {
                            error_mess: Some(&err.to_string()),
                        }
                        .render()
                        .unwrap()
                        .into(),
                    ))
                }
            }
        },
    );
    let index = warp::path::end().map(|| -> warp::reply::Html<String> {
        return warp::reply::html(IndexTemplate { error_mess: None }.render().unwrap().into());
    });

    let routes = versus.or(same).or(index);

    warp::serve(routes).run(([0, 0, 0, 0], port)).await;

    Ok(())
}
