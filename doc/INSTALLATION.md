# Installation and configuration

## Requirements

- Docker
- A YouTube Data API v3 key — create one for free in the
  [Google Cloud Console](https://console.cloud.google.com/apis/library/youtube.googleapis.com):
  create a project, enable the "YouTube Data API v3", then create an API key
  under **Credentials**.

## Run the container

```bash
docker run -d \
  --name yarrtube \
  -e YOUTUBE_API_KEY=your-api-key-here \
  -e PUID=1000 \
  -e PGID=1000 \
  -p 8080:8080 \
  -v /path/on/host/config:/config \
  -v /path/on/host/videos:/videos \
  ghcr.io/sergigp/yarrtube:latest
```

- `-v /path/on/host/videos:/videos` is where downloaded videos will show up
  on your host — point it at your media library.
- `-v /path/on/host/config:/config` holds yarrtube's state: the SQLite
  database (your tracked playlists and channels) and channel avatars. Always
  mount it — otherwise everything you've tracked is lost whenever the
  container is recreated, including on every [update](#updating).
- `PUID`/`PGID` are the numeric user/group ID the daemon runs as and that
  owns downloaded files — usually your own user's (`id -u` / `id -g` on the
  host). Leave them unset to run as root.

Then you can open `http://<YOUR_NAS_IP>:8080/` or `http://localhost:8080/`.

> [!WARNING]
> Yarrtube has no authentication: anyone who can reach the port can use the
> web UI and its API, including the folder browser. Keep it on your LAN, or
> put it behind a reverse proxy that handles authentication.

## Docker Compose

If you run a Docker Compose stack, add:

```yaml
services:
  yarrtube:
    container_name: yarrtube
    mem_limit: 256m
    image: ghcr.io/sergigp/yarrtube:latest
    restart: always
    ports:
      - 8080:8080
    environment:
      - YOUTUBE_API_KEY=your-youtube-data-api-v3-key
      - PUID=1000
      - PGID=1000
      - TZ=Europe/Madrid
    volumes:
      - /path/on/host/config:/config
      - /path/on/host/videos:/videos
```

## Configuration

Environment variables read by Yarrtube are listed below. Every variable except `YOUTUBE_API_KEY` is optional, so you only need to set the ones you want to override:

| Variable                              | Default               | Description                                                                                                                                                                                                   |
| ------------------------------------- | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `YOUTUBE_API_KEY`                     | —                     | YouTube Data API v3 key (required)                                                                                                                                                                            |
| `PUID`                                | `0` (root)            | Numeric user ID the daemon runs as and that owns downloaded files                                                                                                                                             |
| `PGID`                                | `0` (root)            | Numeric group ID the daemon runs as and that owns downloaded files                                                                                                                                            |
| `YARRTUBE_PORT`                       | `8080`                | HTTP port to listen on                                                                                                                                                                                        |
| `YARRTUBE_RECONCILE_INTERVAL_SECONDS` | `3600`                | How often each tracked playlist or channel is reconciled                                                                                                                                                      |
| `YARRTUBE_DB_PATH`                    | `/config/yarrtube.db` | Path to the SQLite database (inside the container)                                                                                                                                                            |
| `YARRTUBE_VIDEOS_PATH`                | `/videos`             | Root directory downloaded videos are saved under (inside the container)                                                                                                                                       |
| `YARRTUBE_DOWNLOAD_CONCURRENCY`       | `2`                   | How many videos download at the same time. Higher values download faster but make YouTube more likely to throttle or bot-check you                                                                            |
| `YARRTUBE_RETRY_BASE_DELAY_SECONDS`   | `150`                 | Delay before retrying a failed task; it grows exponentially with each further retry                                                                                                                           |
| `YARRTUBE_AVATARS_PATH`               | `/config/avatars`     | Directory channel avatars are stored in (inside the container)                                                                                                                                                |
| `YARRTUBE_SPONSORBLOCK_REMOVE` | empty (disabled) | SponsorBlock removal categories: `default`, `all`, or explicit comma-separated categories (see below). |
| `YARRTUBE_WRITE_SUBS` | `false` | Download creator-provided captions in the selected languages. Converted to SRT before optional embedding. |
| `YARRTUBE_WRITE_AUTO_SUBS` | `false` | Download automatic captions and automatic translations in the selected languages. Creator captions take precedence for the same language when both switches are enabled. |
| `YARRTUBE_EMBED_SUBS` | `true` | Embed downloaded captions after SponsorBlock timing correction, with titles such as `English-SRT` or `English-GENERATED-SRT`. Remove external SRTs after successful embedding; `false` keeps external SRTs only. |
| `YARRTUBE_SUB_LANGS` | `en.*` | Comma-separated yt-dlp language codes/patterns. `en.*` selects English variants only; `en.*,de.*` selects English and German. Live chat is always excluded. |
| `YTDLP_PATH`                          | `/app/bin/yt-dlp`     | Path to the managed `yt-dlp` binary (also the path bundled into the image at build time)                                                                                                                      |
| `RUST_LOG`                            | `info`                | Log verbosity (e.g. `RUST_LOG=debug`)                                                                                                                                                                         |
| `YARRTUBE_PLEX_URL`                   | —                     | Base URL of your Plex server (e.g. `http://192.168.1.10:32400`). With the token and `YARRTUBE_PLEX_VIDEOS_PATH`, enables the [Plex scan trigger](#plex)                                                       |
| `YARRTUBE_PLEX_TOKEN`                 | —                     | Plex authentication token (`X-Plex-Token`)                                                                                                                                                                    |
| `YARRTUBE_PLEX_VIDEOS_PATH`           | —                     | Where the Plex server sees yarrtube's videos root (`YARRTUBE_VIDEOS_PATH`), e.g. the host path `/volume1/data/media/yarrtube` mounted at `/videos`. Plex is asked to scan a show's folder after each download |

## SponsorBlock

Set `YARRTUBE_SPONSORBLOCK_REMOVE=default` on the container to remove SponsorBlock
segments from new downloads. This removes sponsorships, intros, outros,
self-promotion, previews, interaction reminders, non-music sections, and hooks;
filler tangents are preserved. Use `all` to include filler, or a comma-separated
list such as `sponsor,selfpromo,interaction` to choose categories explicitly.
An empty or unset value disables SponsorBlock. Invalid categories fail startup.

For Compose, add this under the service environment and recreate the container:

```yaml
- YARRTUBE_SPONSORBLOCK_REMOVE=default
```

No additional API key, container, or database migration is needed. yt-dlp queries
the public SponsorBlock service and FFmpeg cuts matching segments before the
video reaches the library. Videos without matching segments remain uncut.
The feature depends on community-submitted timestamps and is not guaranteed
to remove every sponsored segment. Existing files are not reprocessed.

Cuts use yt-dlp's default behaviour without forcing a full re-encode; minor
artifacts near cut boundaries are possible. SponsorBlock lookup warnings are
logged; SponsorBlock lookup, cutting, and download failures use the existing
task retry mechanism rather than silently delivering an uncut video.

## Subtitles

Subtitle downloading is optional and disabled by default. Add these environment
variables to the container to download creator captions and automatic captions:

```yaml
- YARRTUBE_WRITE_SUBS=true
- YARRTUBE_WRITE_AUTO_SUBS=true
- YARRTUBE_SUB_LANGS=en.*
- YARRTUBE_EMBED_SUBS=true
```

The two switches are independent (`true` or `false`). `YARRTUBE_SUB_LANGS`
accepts yt-dlp language codes and patterns separated by commas: `en.*` (default),
`en.*,de.*`, or `all`. Live chat is always excluded. When both sources offer the
same language, yt-dlp prefers creator-provided captions. Missing captions or no
matching languages do not by themselves fail the video download. Subtitle network
or conversion errors follow the existing download retry mechanism.

Subtitles are converted to SRT in the download staging folder. Embedding is
enabled by default: after successful embedding, external SRTs are removed and
only the video with its embedded tracks reaches the library. Set
`YARRTUBE_EMBED_SUBS=false` for external-only language-tagged files, such as
`Season 1/S01E001 - A Video.en.srt`, next to the video. Downloading still uses
the two opt-in switches above.

Embedded track titles use full English language names: `English-SRT` for creator
captions and `English-GENERATED-SRT` for automatic captions (including automatic
translations). ISO language tags are also set. The SRT label describes the input
format; MP4 stores these tracks as compatible timed text. Embedding runs after
SponsorBlock timestamp repair, copies the
video/audio without re-encoding, and leaves subtitle playback selection to the
player. Sidecars are moved/renamed with the episode, retained during orphan
cleanup, and removed with the video. Redownloading replaces old sidecars, since
new SponsorBlock cuts could make old timestamps incorrect. Existing downloaded
videos are not automatically fetched again just because these settings change.

With SponsorBlock enabled, Yarrtube adjusts original SRT timestamps using the
SponsorBlock cut ranges. Original intermediate files stay in staging until the
download is placed, then are deleted. This avoids FFmpeg dropping sparse cues. Recreate the container after changing these
settings; selecting many auto-translated languages can increase YouTube requests.

## Videos on disk

Each playlist and channel is saved as a TV show (layout in
[PLEX.md](PLEX.md#what-you-get)). Keep the videos root on a single
filesystem: downloads are staged in a hidden `.downloads` folder there and
moved into place when complete.

## Plex

Point a Plex **TV Shows** library (Plex NFO Series agent) at your videos
folders. Optionally, set `YARRTUBE_PLEX_URL`, `YARRTUBE_PLEX_TOKEN` and
`YARRTUBE_PLEX_VIDEOS_PATH` (all three) and yarrtube asks Plex to scan a
show's folder as soon as one of its videos is downloaded. The library setup
and the scan trigger are covered step by step in [PLEX.md](PLEX.md).

## Updating

> [!NOTE]
> Updating from 0.8.x or older? 0.9.0 moved the library to a TV show layout
> and changed the Plex settings. Follow
> [UPGRADING-TO-0.9.0.md](UPGRADING-TO-0.9.0.md) once instead.

With `docker run`:

```bash
docker pull ghcr.io/sergigp/yarrtube:latest
docker stop yarrtube && docker rm yarrtube
# then re-run the `docker run` command above
```

With Docker Compose:

```bash
docker compose pull yarrtube
docker compose up -d yarrtube
```


## Download and subtitle troubleshooting logs

Yarrtube writes timestamped logs to the container output. View them in your
container manager's **Logs** view, or run these commands on the Docker host
(replace the container name if yours differs):

```sh
docker logs --since 1h --timestamps sergigp-yarrtube
docker logs --follow --since 5m --timestamps sergigp-yarrtube
```

After reproducing a failed subtitle download, export a file you can share:

```sh
docker logs --since 1h --timestamps sergigp-yarrtube > yarrtube.log 2>&1
```

At the default `RUST_LOG=info` level, each download logs its effective creator,
automatic, embedding, language and SponsorBlock settings; available and requested
caption language codes; yt-dlp warnings even when the video download succeeds;
SRT file counts and sizes; timing correction; and embedding track titles and
completion. The `staging_dir` field correlates messages from concurrent downloads.
`none` means an empty language inventory, while `unknown` means the tool did not
provide readable metadata. Missing SRTs emit a warning rather than silently
skipping embedding. Failed stages also appear in the task's error details.

New yt-dlp diagnostic log lines redact URL-bearing tokens and the configured
YouTube API key, and bound the output size. Raw caption metadata, signed caption
URLs, cookies and the full process command are not dumped by these diagnostics.
Review exported logs before sharing: other application messages can include
video titles, local paths and source identifiers.

These logs cover **new download attempts**. Changing subtitle settings does not
retroactively process videos already downloaded. Pull the updated image and
recreate the container, then retry a download or add a new video before exporting.
Docker retains the logs according to the host's logging driver and rotation
settings; no additional Yarrtube log volume or environment variable is needed.
