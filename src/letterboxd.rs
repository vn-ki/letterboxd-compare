use anyhow::{anyhow, Result};
use core::hash::{Hash, Hasher};
use futures::TryStreamExt;
use futures::{stream, StreamExt};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::fmt::Formatter;
use tracing::{debug, info};
// use tokio_stream::{self as stream, StreamExt};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LetterboxdError {
    #[error("missing attr {0}")]
    HtmlMissingAttr(String),

    #[error("user not found {0}")]
    UserNotFound(String),

    #[error("Error while getting the number of pages")]
    PaginationElementNotFound,

    #[error("parsing failed: {0:?}")]
    ParseError(#[from] std::num::ParseIntError),
}

pub struct LetterboxdClient {
    client: reqwest::Client,
}

#[derive(Copy, Clone, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq)]
pub struct Rating(u8);

impl std::fmt::Debug for Rating {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}", (self.0 as f64) / 2.0))
    }
}

impl std::fmt::Display for Rating {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let star = match self.0 {
            1 => "½",
            2 => "★",
            3 => "★½",
            4 => "★★",
            5 => "★★½",
            6 => "★★★",
            7 => "★★★½",
            8 => "★★★★",
            9 => "★★★★½",
            10 => "★★★★★",
            _ => {
                debug!("got no rating: {}", self.0);
                "no rating"
            }
        };
        f.write_fmt(format_args!("{}", star))
    }
}

impl From<usize> for Rating {
    fn from(t: usize) -> Self {
        Rating(t as u8)
    }
}

#[derive(Debug, Serialize, Deserialize, Eq)]
pub struct Film {
    pub id: u64,
    pub name: String,
    pub url: String,
    pub poster: String,
    /// rating is from 0-10
    pub rating: Option<Rating>,
}

impl Hash for Film {
    fn hash<H>(&self, h: &mut H)
    where
        H: Hasher,
    {
        self.id.hash(h)
    }
}

impl PartialEq for Film {
    fn eq(&self, rhs: &Film) -> bool {
        self.id == rhs.id
    }
}

impl LetterboxdClient {
    pub fn new() -> Self {
        LetterboxdClient {
            client: reqwest::Client::new(),
        }
    }

    fn parse_rating(rating: &str) -> Result<Rating> {
        Ok(match rating {
            "½" => 1,
            "★" => 2,
            "★½" => 3,
            "★★" => 4,
            "★★½" => 5,
            "★★★" => 6,
            "★★★½" => 7,
            "★★★★" => 8,
            "★★★★½" => 9,
            "★★★★★" => 10,
            // TODO: Error here
            _ => return Err(anyhow!("unknown rating: '{}'", rating)),
        }
        .into())
    }

    fn get_pages(&self, html: &Html) -> std::result::Result<usize, LetterboxdError> {
        let pagination_sel = Selector::parse("div.pagination").unwrap();
        let li_sel = Selector::parse("li.paginate-page > a").unwrap();
        let page = match html.select(&pagination_sel).next() {
            Some(p) => p,
            // couldn't find the pagination thingy, must mean that only 1 page?
            None => return Ok(1),
        };

        let no_pages = page
            .select(&li_sel)
            .last()
            .ok_or(LetterboxdError::PaginationElementNotFound)?;
        Ok(no_pages.inner_html().parse()?)
    }

    fn film_from_elem_ref(&self, movie: &scraper::ElementRef) -> Result<Film> {
        let data_selector = Selector::parse("div.react-component").unwrap();
        let poster_selector = Selector::parse("img").unwrap();
        let rating_selector = Selector::parse("span.rating").unwrap();

        let data = movie.select(&data_selector).next().unwrap().value();
        // poster is inside
        let poster = movie.select(&poster_selector).next().unwrap().value();
        let rating = movie
            .select(&rating_selector)
            .next()
            .map(|r| Self::parse_rating(r.text().next().unwrap()))
            .transpose()?;
        Ok(Film {
            id: data
                .attr("data-film-id")
                .ok_or(LetterboxdError::HtmlMissingAttr("data-film-id".into()))?
                .parse()?,
            name: poster
                .attr("alt")
                .ok_or(LetterboxdError::HtmlMissingAttr("alt".into()))?
                .into(),
            url: format!(
                "https://letterboxd.com/film/{}",
                data.attr("data-item-slug")
                    .ok_or(LetterboxdError::HtmlMissingAttr("data-item-slug".into()))?
            ),
            poster: poster
                .attr("src")
                .ok_or(LetterboxdError::HtmlMissingAttr("src".into()))?
                .into(),
            rating,
        })
    }

    #[tracing::instrument(skip(self))]
    async fn get_letterboxd_film_by_page(
        &self,
        username: &str,
        page: usize,
    ) -> Result<reqwest::Response> {
        let url = format!("https://letterboxd.com/{}/films/page/{}", username, page);
        debug!("fetching url={}", url);
        let text = self.client.get(&url).send().await?;
        Ok(text)
    }

    pub async fn get_movies_from_page(&self, username: &str, page: usize) -> Result<Vec<Film>> {
        let text = self
            .get_letterboxd_film_by_page(username, page)
            .await?
            .text()
            .await?;
        let document = Html::parse_document(&text);
        let selector = Selector::parse("li.griditem").unwrap();

        document
            .select(&selector)
            .map(|movie| self.film_from_elem_ref(&movie))
            .collect()
    }

    #[tracing::instrument(skip(self))]
    pub async fn get_movies_of_user(&self, username: &str) -> Result<Vec<Film>> {
        // let text = self.get_letterboxd_film_by_page(username, 1).await?;
        // let document = Html::parse_document(&text);
        // let no_of_pages = self.get_pages(&document)?;
        // TODO: this is weird async problem
        let no_of_pages = {
            let resp = self.get_letterboxd_film_by_page(username, 1).await?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(LetterboxdError::UserNotFound(username.into()).into());
            }
            let text = resp.text().await?;
            let document = Html::parse_document(&text);
            self.get_pages(&document)
        }?;
        info!(no_of_pages = no_of_pages);

        let films: Vec<Film> = stream::iter(1..=no_of_pages)
            .map(|i| self.get_movies_from_page(username, i))
            .buffer_unordered(5)
            .try_concat()
            .await?;

        info!(films_len = films.len());
        Ok(films)
    }
}
