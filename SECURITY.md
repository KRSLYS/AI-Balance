# Security

Please report a vulnerability privately, not in a public issue.

- Email <info@krslys.be>, or
- use **Report a vulnerability** under the repository's Security tab.

Include what you found, how to reproduce it, and the version of AI Balance. You will get an answer
within seven days. Please give us a reasonable time to fix the problem before you publish it.

Only the latest release receives security fixes.

## How the app protects your accounts

- **Credentials stay on the device.** On desktop they are kept in the operating system's keychain;
  on phones, in storage only the app can read. The app has no server to send them to.
- **Each credential goes only to its own provider,** and only over HTTPS: the app refuses plain HTTP,
  even when an address or a redirect asks for it.
- **The least access a sign-in can ask for.** Where a provider offers scopes, the app requests only
  what reading usage needs.
- **Sign-in answers are checked.** A browser sign-in must return the one-time value the app sent
  (PKCE and state), a sign-in page is opened only when its address is HTTPS, and a server address
  that arrives in an answer is used only when it belongs to that provider.
- **Nothing sensitive is written to logs,** and the interface loads no code or content from the
  network.
- **Dependencies are audited** every week and whenever they change.
