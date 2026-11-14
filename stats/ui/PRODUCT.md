# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

React with dither-kit (Boring-Software-Inc/dither-kit, its components copied in the shadcn way) and Tailwind, built to static files that the stats service serves from its own origin. Chosen by coah on 2026-11-14 over a no-script page.

## Users

One person: coah, who makes framecorder. He opens it on a phone and on a desktop monitor about equally, to see how framecorder is doing. Nobody else can get in.

## Product Purpose

The private dashboard of framecorder's stats service (`db.coah80.com`). It shows the daily totals the service keeps: how many installs of the headset tab, the desktop app and the Android app ran each day, how many were new, which versions and systems they are, how many recordings and clips were made, how ten headset settings stand, where visits to the Discord link came from, and how often the dashboard's own login was tried.

The first thing it has to answer: is framecorder growing. How many installs ran today, and how that compares with last week, across the three surfaces.

## Positioning

Every number is an anonymous daily total. There is no per-install, per-device or per-address data anywhere in the service, so the dashboard can never show an individual.

## Operating Context

Read-only. Opened for a look, a few times a week, sometimes right after a release to watch a version spread. Data arrives once a day per install, so nothing on it changes minute to minute.

## Capabilities and Constraints

- Data: `GET /api/stats` returns the last 90 days as `{today, days: {day: {counter: n}}}`. Counters: `active:<app>`, `new:<app>`, `version:<app>:<v>`, `os:<app>:<os>`, `count:recordings`, `count:clips`, `setting:<key>=<value>`, `discord:from=<source>`, `discord:country=<cc>`, `auth:in|wrong|locked`. Apps are `headset`, `desktop`, `android`.
- Before the first release ships with the ping, every chart is empty. Empty is a real state, not an error.
- The page is served under a content security policy that allows scripts, styles, fonts and images from its own origin only: nothing inline, nothing from a CDN.
- The login page is separate, server-rendered and script-free.

## Brand Commitments

framecorder's look, from the repo's AGENTS.md: dark, mauve `#cba6f7`, Montserrat for headings, Poppins for text, Space Grotesk for numbers. coah asked for dither-kit's dithered charts on this surface.

## Evidence on Hand

Real data only. Until installs report in, the dashboard shows what little there is; a sample data file exists for development and must never be served.

## Product Principles

1. Growth first: the top of the page answers "is it growing" before anything else.
2. Totals, never people: nothing is drawn that could suggest an individual install.
3. Works the same in the hand and on the desk.
4. Honest when empty or thin: small numbers look small.
