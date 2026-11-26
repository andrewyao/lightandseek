# lightandseek
Hide and seek game for multiple players

## Running

`just serve` builds the web client and serves it with the room server on
port 8080.

The public build lives at andrewyao.app/seek/. `scripts/deploy-web.sh` builds
the client into the andrewyao.app site repo's `public/seek` and deploys the
rooms Worker in `crates/cloudflare`. It does not commit or push the site.
