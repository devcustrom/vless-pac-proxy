# Third-party notices

## tg-ws-proxy

This directory contains a modified headless subset of
[Flowseal/tg-ws-proxy](https://github.com/Flowseal/tg-ws-proxy) v1.11.4.

MIT License

Copyright (c) 2026 Flowseal

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Zaprett catalog (external, not bundled)

The Zapret page can load strategies, lists, ipsets and binary payloads from an
external catalog, by default:

https://github.com/CherretGit/zaprett-repo

Those files are **not** part of wireproxy-control and are not redistributed in
its releases. They are downloaded at runtime only after an explicit user
action; every artifact is verified by sha256. The catalog repository does not
contain an explicit license file, so check the author's terms before
redistributing its contents. Authors and source URLs are shown in the UI for
every item.

