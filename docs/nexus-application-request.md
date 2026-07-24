# Nexus Mods application id request

Send to **support@nexusmods.com**. Ask for the Community Team, since they are
the ones who issue application references.

Attach before sending:

- A high resolution logo (PNG, square, legible on a dark background).
- A test build, or a link to one. A GitHub release or an AppImage is fine.

---

**Subject:** Application ID request for Apocrypha, an open source Linux mod manager

---

Hello,

I am writing to request an application reference for Nexus Mods SSO, and to
introduce the project it is for.

**What Apocrypha is**

Apocrypha is a free, open source mod manager built specifically for Linux. Most
existing managers are Windows applications that Linux players run through Wine
or Proton, or that cannot work on Linux at all: Mod Organizer 2 is built on
USVFS, a user space virtual filesystem made from Windows DLL injection, which
has no Linux equivalent. The practical result is that many Linux players end up
copying mod files into their game folder by hand and hoping they can undo it
later.

Apocrypha is written from that starting point rather than ported to it. It
understands Steam and Proton natively, keeps every staged file outside the game
directory, and treats each deployment as a journaled transaction that can be
reversed file by file. The first supported game is Monster Hunter Wilds,
including the REFramework loader setup that Proton otherwise makes awkward.

- Repository: https://github.com/Ali-AbdulHadii/apocrypha
- Licence: MIT
- Platform: Linux (x86_64)
- Status: early development, version 0.1

**What I am asking for**

An application reference for the SSO flow at `wss://sso.nexusmods.com`, so users
can approve access in their browser instead of copying a personal API key by
hand. The flow is already implemented against your documented protocol and the
`sso-integration-demo` repository. It is disabled in the interface until an
application reference exists, and pasting a personal API key remains available
either way.

If OAuth is now preferred over SSO for third party applications, I am happy to
implement that instead. Please let me know which you would rather I use.

**Request metadata I send**

Every API request carries:

```
Application-Name: Apocrypha
Application-Version: 0.1.0
```

**How I handle the API and user data**

I have read the API Acceptable Use Policy and built to it deliberately:

- API keys are stored only on the user's own machine. Nothing is sent to any
  server of mine, and there is no server of mine involved in authentication.
- Requests are only ever made in response to a direct user action. There is no
  background polling and no bulk fetching.
- No Nexus Mods data is scraped, mirrored, or rehosted.
- No personal API key is embedded in the application.
- Rate limits are read from the `X-RL-*` response headers rather than assumed,
  and requests stop when the remaining quota reaches zero rather than retrying
  into a block.
- Free accounts are handled the way your documentation requires: the application
  never attempts a premium only download path. When a download needs a token, it
  opens the mod page so the user can use the Mod Manager Download button, and
  the resulting `nxm://` link supplies the key and expiry. This is presented to
  users as how Nexus Mods works, not as a limitation of the application.

**Also useful to know**

Apocrypha registers itself as a handler for `nxm://` links on Linux through a
standard desktop entry, so the Mod Manager Download button works as users
expect.

I am happy to provide a build, a walkthrough, or anything else that would help
with review. Thank you for your time, and for keeping the API open to
third party applications.

Best regards,

Ali Abdulhadi
https://github.com/Ali-AbdulHadii/apocrypha
