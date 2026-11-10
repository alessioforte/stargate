# Stargate transactional emails

React Email templates for Stargate's transactional messages. A build script
renders every template for every supported locale into plain HTML that the
server reads from `.stargate/transactional/<template-name>.<locale>.html`.

## Templates

Every `Template` variant in `crates/smtp/src/smtp.rs` must have a matching
`emails/<template-name>.tsx` with a default export and a subject in the
dictionary:

| Template variant              | File                          | Placeholders              |
| ----------------------------- | ----------------------------- | ------------------------- |
| `SignupRequest`               | `signup-request.tsx`          | `{{link}}`                |
| `SignupAccountExists`         | `signup-account-exists.tsx`   | `{{link}}`                |
| `SignupCompleted`             | `signup-completed.tsx`        | —                         |
| `ChangePasswordRequest`       | `change-password-request.tsx` | `{{link}}`                |
| `PasswordChangedNotification` | `password-changed.tsx`        | —                         |
| `EmailOtp`                    | `email-otp.tsx`               | `{{code}}`, `{{minutes}}` |

The server replaces `{{link}}` with the action URL, `{{name}}` with the
recipient's name when available, and the OTP's `{{code}}` / `{{minutes}}`.
Every substituted value is HTML-escaped by `crates/smtp`.

`EmailOtp` is sent by `src/iam/otp/delivery.rs` for passwordless login and MFA
challenges. Its expiry wording lives in the template; the server only passes the
number of minutes.

## Languages

`emails/_components/i18n.ts` is the single source for copy and subjects:

- `en` defines every key; `it` is typed as `Record<CopyKey, string>`, so a
  missing translation is a TypeScript error.
- `templateIds` lists the templates the build script renders.
- `buildSubjects()` produces the `subjects.json` the server reads; startup fails
  if a template file or a subject is missing for any locale.

Supported locales are `en` and `it`. The server's `DEFAULT_LOCALE` picks the
default when the recipient has no saved language.

## Shared design

All templates use the same shell from `emails/_components/`:

- `layout.tsx`: branded header, content card, and footer.
- `kit.tsx`: primary button, plain-link fallback, and note boxes.
- `theme.ts`: colors, typography, and spacing tokens.
- `i18n.ts`: localized copy, subjects, and locale helpers.

Keep every template file with an `export default`. `_components` is ignored by
the React Email dev server.

## Getting Started

First, install the dependencies:

```sh
npm install
# or
yarn
```

Then, run the development server (English previews):

```sh
npm run dev
# or
yarn dev
```

Open [localhost:3000](http://localhost:3000) with your browser to see the result.

Render every template for every locale, plus `subjects.json`:

```sh
npm run export
```

## License

Stargate's email templates are licensed under the
[Elastic License 2.0 (ELv2)](../../LICENSE). React Email and other third-party
dependencies retain their original licenses.
