# cloudflare

The Cloudflare port of `crates/server`: a Worker routed at
`andrewyao.app/seek/ws/*`, with one Durable Object per room. The client is
served as static files by the andrewyao.app site.

A room lives in the object's memory from its first join until it has been
empty for 15 seconds, the same linger as the tokio server, so a reload
resumes the round. Nothing is stored. The object is SQLite-backed only
because that is the recommended backend and the one the free plan allows.

A socket that stops reading is dropped once its unsent bytes
(`bufferedAmount`) pass 256 KiB, the stand-in for the tokio server's
64-frame queue. That count only starts to grow once the buffers between
the object and the client are full. Under `wrangler dev` about 1 MB went
downstream first, which at snapshot rate (about 1 KB/s) is many minutes, so
expect a stalled client to be dropped much later than on the tokio server.

`scripts/deploy-web.sh` builds the client into that site's `public/seek` and
deploys this Worker. `wrangler dev` here runs it locally at
`ws://localhost:8787/seek/ws/{room}`.
