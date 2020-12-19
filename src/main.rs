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
use std::collections::HashSet;
use tracing::{debug, info};
use tracing_subscriber;
use warp::Filter;

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {}

#[derive(Template)]
#[template(path = "diff.html")]
struct DiffTemplate<'a> {
    user1: &'a str,
    user2: &'a str,
    diff: Vec<Film>,
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
    let movies1 = cached_get_movies(user1).await?;
    let movies2 = cached_get_movies(user2).await?;

    let watched_by_2: HashSet<_> = movies2.into_iter().map(|x| x.id).collect();

    let mut diff: Vec<_> = movies1
        .into_iter()
        .filter(|x| !watched_by_2.contains(&x.id))
        .collect();
    diff.sort_by(|a, b| match (a.rating, b.rating) {
        (Some(r1), Some(r2)) => r2.partial_cmp(&r1).unwrap(),
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (None, None) => Ordering::Equal,
    });
    Ok(DiffTemplate {
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
    let hello =
        warp::path!(String / "vs" / String).and_then(async move |user1: String, user2: String| {
            match get_diff(&user1, &user2).await {
                Ok(s) => Ok(warp::reply::html(s)),
                Err(_) => return Err(warp::reject::not_found()),
            }
        });
    let index = warp::path::end().map(|| -> warp::reply::Html<String> {
        return warp::reply::html(IndexTemplate {}.render().unwrap().into());
    });

    let routes = hello.or(index);

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;

    Ok(())
}
