use anyhow::{anyhow, Result};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::fmt::Formatter;
use tracing::debug;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LetterboxdError {
    #[error("missing attr {0}")]
    HtmlMissingAttr(String),

    #[error("user not found {0}")]
    UserNotFound(String),
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
        // f.write_fmt(format_args!("{}", (self.0 as f64) / 2.0));
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
            _ => "no rating",
        };
        f.write_fmt(format_args!("{}", star))
    }
}

impl From<usize> for Rating {
    fn from(t: usize) -> Self {
        Rating(t as u8)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Film {
    pub id: u64,
    pub name: String,
    pub url: String,
    pub poster: String,
    /// rating is from 0-10
    pub rating: Option<Rating>,
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

    fn get_pages(&self, html: &Html) -> Result<usize> {
        let pagination_sel = Selector::parse("div.pagination").unwrap();
        let li_sel = Selector::parse("li.paginate-page > a").unwrap();
        let page = match html.select(&pagination_sel).next() {
            Some(p) => p,
            // couldn't find the pagination thingy, must mean that only 1 page?
            None => return Ok(1),
        };

        let no_pages = page.select(&li_sel).last().unwrap();
        Ok(no_pages.inner_html().parse()?)
    }

    fn film_from_elem_ref(&self, movie: &scraper::ElementRef) -> Result<Film> {
        let data_selector = Selector::parse("div.film-poster").unwrap();
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
                "https://letterboxd.com{}",
                data.attr("data-film-slug")
                    .ok_or(LetterboxdError::HtmlMissingAttr("data-film-slug".into()))?
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
        debug!(no_of_pages = no_of_pages);

        let selector = Selector::parse("li.poster-container").unwrap();

        let mut curr_page = 1;
        let mut films: Vec<Film> = Vec::with_capacity(no_of_pages * 12 * 6);

        // TODO: for future, can be async. use a channel
        loop {
            // get the next
            let text = self
                .get_letterboxd_film_by_page(username, curr_page)
                .await?
                .text()
                .await?;
            let document = Html::parse_document(&text);
            for movie in document.select(&selector) {
                films.push(self.film_from_elem_ref(&movie)?);
            }
            curr_page += 1;
            if curr_page > no_of_pages {
                break;
            }
        }
        debug!(films_len = films.len());
        Ok(films)
    }
}
