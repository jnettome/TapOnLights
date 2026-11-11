# Security policy

TapOnLights stores your Tapo password in Windows Credential Manager and can serve a web page and API on your network, so security reports are very welcome.

## Reporting a vulnerability

Please report it privately through GitHub: open the **Security** tab of this repository and choose **Report a vulnerability**. Don't open a public issue for security problems.

Include what you found, how to reproduce it, and the app version. You'll get a reply as soon as possible.

## Scope

In scope:

- the app (`src-tauri/`, `src/`), including the optional web server and its API;
- how credentials and settings are stored;
- the `tools/tapo-probe` utility.

Out of scope:

- the bulbs' own protocol and firmware: report those to TP-Link;
- the `tapo` library: report those to [its project](https://github.com/mihai-dinculescu/tapo).

## Things to know

- The web page is plain HTTP and is off by default. With network access on, use it only on networks you trust and set a password.
- Release builds aren't code-signed yet.
