# Letterboxd Compare

Compare two letterboxd users to see the movies one has seen and the other hasn't.
[Letterboxd](https://letterboxd.com) is a social movie discovery site where users rate the movies they watch.

The initial fetch of data might take a couple of seconds, especially if the requested user has rated a lot of movies.

## Instructions

- Needs rust developement setup

```
cargo run
```

For developement, you might want logs

```
RUST_LOG="info,letterboxd_compare=debug" cargo run
```
