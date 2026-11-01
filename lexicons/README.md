# Lexicons

Every Local Network tool's lexicons are under `computer/`, each in the folder its NSID names. Those are the ones we publish, format, lint and check for breaking changes.

The other folders hold other authorities' lexicons that ours refer to or that our tools write, copied in as their authorities publish them so nothing has to fetch them at runtime. They're theirs, so we don't edit, format, lint or publish them. `tools/lexicons.sh pull` fetches every one of them again as published now, and `tools/lexicons.sh pull <nsid>` adds another.

| NSID                                 | Authority DID                      | Record CID                                                    | Fetched    |
| ------------------------------------ | ---------------------------------- | ------------------------------------------------------------- | ---------- |
| `community.lexicon.location.address` | `did:plc:mtr7qrqtcyseedx3jyr5o7db` | `bafyreicdvexolyvp6j6yksqiib7hihwktt6ogalbvyzvtkj6ecrtqqw5fq` | 2026-10-24 |
| `community.lexicon.location.geo`     | `did:plc:mtr7qrqtcyseedx3jyr5o7db` | `bafyreiansycpa3ggyxqnttzb7ir3dw654s2vk7nldpss6jz65m5zh5frzu` | 2026-10-24 |
| `community.lexicon.location.fsq`     | `did:plc:mtr7qrqtcyseedx3jyr5o7db` | `bafyreigqx7wxb4cmz3aex6pcjs6vwqwaqldzn3xvrml3yrpq623u6cnjpe` | 2026-10-24 |
| `community.lexicon.location.hthree`  | `did:plc:mtr7qrqtcyseedx3jyr5o7db` | `bafyreic6xmiv4ycbcfvcvj5uu537ytl7hnjazcxf7hhd7g6o6s7p6tonky` | 2026-10-24 |
| `community.lexicon.calendar.event`   | `did:plc:mtr7qrqtcyseedx3jyr5o7db` | `bafyreigv256whv46vep4xb2iqn6ag7z5abhvebd46ohuqvopxl56ptobbi` | 2026-10-24 |
| `com.atproto.repo.strongRef`         | `did:plc:6msi3pj7krzih5qxqtryxlzw` | `bafyreifrkdbnkvfjujntdaeigolnrjj3srrs53tfixjhmacclps72qlov4` | 2026-10-24 |
| `app.userinput.discussion`           | `did:plc:uyixj57k6nmxrdj7pjs2ss5s` | `bafyreia3pm5ywlis5lzl2lokic57wbsr4cez4tkrc7xicf6i76a72j7ey4` | 2026-10-24 |
| `app.userinput.space`                | `did:plc:uyixj57k6nmxrdj7pjs2ss5s` | `bafyreifzxvmlucnnqfxkbxq72ledmrjhdvt5ry3fqlpkacl7nmi6fnaohy` | 2026-10-24 |
| `com.atproto.label.defs`             | `did:plc:6msi3pj7krzih5qxqtryxlzw` | `bafyreidp2wpcbzl2qiob2uwoc7lhntojx3tjcr553mmahw2f5cumtm3wpm` | 2026-10-24 |
| `app.bsky.actor.profile`             | `did:plc:4v4y5r3lwsbtmsxhile2ljac` | `bafyreifmvpqm57xys7vaezajca44ejnkmd6juvsuflvjatkq3gb62nnnoy` | 2026-10-24 |
| `app.bsky.graph.follow`              | `did:plc:4v4y5r3lwsbtmsxhile2ljac` | `bafyreiav7u7jchqtsxr2yy3gsazfqqziw5tcrfniuprrmoariruj3cft5u` | 2026-10-24 |
| `app.bsky.graph.verification`        | `did:plc:4v4y5r3lwsbtmsxhile2ljac` | `bafyreidn7abfzt5p4gcujys2w4seigbhcfnscos6fmh4hnce2gdormbwpa` | 2026-10-24 |

After a pull, update this table: each authority's `_lexicon` DNS record names its DID, and the CID is that of its `com.atproto.lexicon.schema` record with the NSID as its key.
