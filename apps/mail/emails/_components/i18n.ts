export const locales = ["en", "it"] as const;
export type Locale = (typeof locales)[number];

export const templateIds = [
  "signup-request",
  "signup-account-exists",
  "signup-completed",
  "change-password-request",
  "password-changed",
  "email-otp",
] as const;
export type TemplateId = (typeof templateIds)[number];

const en = {
  "common.brandTagline": "Identity & access",
  "common.footerTagline": "Secure identity and access management.",
  "common.footerLegal": "© Stargate Inc. All rights reserved.",
  "common.footerMeta": "This is an automated message. Please do not reply.",
  "common.fallbackLabel": "Button not working?",

  "signup-request.subject": "Confirm your Stargate email",
  "signup-request.preview":
    "Confirm your email address to finish setting up your Stargate account.",
  "signup-request.eyebrow": "Account verification",
  "signup-request.title": "Confirm your email address",
  "signup-request.lead":
    "We received a request to create a Stargate account for this email address.",
  "signup-request.body":
    "Confirm your address to continue. The link below applies to this request only and can be used once.",
  "signup-request.button": "Verify email address",
  "signup-request.noteTitle": "Didn't request this?",
  "signup-request.note":
    "No account has been created yet. If you weren't expecting this email, you can safely ignore it and nothing further will happen.",

  "signup-account-exists.subject": "You already have an account",
  "signup-account-exists.preview":
    "An account already uses this email address. Sign in to continue.",
  "signup-account-exists.eyebrow": "Account notice",
  "signup-account-exists.title": "You already have an account",
  "signup-account-exists.lead":
    "Someone requested a Stargate account with this email address, but an account already exists, so we didn't create a new one.",
  "signup-account-exists.body":
    "Sign in below to continue. If you've forgotten your password, choose “Forgot password” on the sign-in page.",
  "signup-account-exists.button": "Sign in to Stargate",
  "signup-account-exists.noteTitle": "Your account hasn't changed",
  "signup-account-exists.note":
    "If you didn't make this request, no action is needed. Your sign-in details remain exactly as they were.",

  "signup-completed.subject": "Welcome to Stargate!",
  "signup-completed.preview": "Your Stargate account is ready to use.",
  "signup-completed.eyebrow": "Welcome",
  "signup-completed.title": "Welcome to Stargate",
  "signup-completed.lead":
    "Your account has been created successfully. You can now sign in with your email address and password.",
  "signup-completed.body": "We're glad to have you on board.",
  "signup-completed.noteTitle": "Getting started",
  "signup-completed.note":
    "Sign in with your email address and password. If your organization requires multi-factor authentication, you'll be guided through it on your next sign-in.",

  "change-password-request.subject": "Change Password Request",
  "change-password-request.preview":
    "Choose a new password for your Stargate account.",
  "change-password-request.eyebrow": "Password reset",
  "change-password-request.title": "Reset your password",
  "change-password-request.lead":
    "We received a request to reset the password for your Stargate account.",
  "change-password-request.body":
    "Choose a new password with the button below. The link expires in one hour and can be used once.",
  "change-password-request.button": "Choose a new password",
  "change-password-request.noteTitle": "Didn't request this?",
  "change-password-request.note":
    "Your password won't change unless you use the link above. You can safely ignore this email.",

  "password-changed.subject": "Password Changed",
  "password-changed.preview":
    "The password for your Stargate account was changed.",
  "password-changed.eyebrow": "Account security",
  "password-changed.title": "Your password was changed",
  "password-changed.lead":
    "The password for your Stargate account was changed successfully.",
  "password-changed.body": "Use your new password the next time you sign in.",
  "password-changed.noteTitle": "Didn't change your password?",
  "password-changed.note":
    "Reset your password immediately. If you can no longer access your account, contact your organization's administrator.",

  "email-otp.subject": "Your verification code",
  "email-otp.preview": "Your Stargate verification code",
  "email-otp.eyebrow": "Verification code",
  "email-otp.title": "Your verification code",
  "email-otp.lead":
    "Enter the code below to sign in or complete your verification. It can be used once.",
  "email-otp.expiry": "The code expires in {{minutes}} min.",
  "email-otp.noteTitle": "Never share this code",
  "email-otp.note":
    "Stargate will never ask you to share this code. If you didn't request it, you can safely ignore this email.",
} as const;

export type CopyKey = keyof typeof en;
export type Copy = Record<CopyKey, string>;

const it: Copy = {
  "common.brandTagline": "Identità e accesso",
  "common.footerTagline": "Gestione sicura delle identità e degli accessi.",
  "common.footerLegal": "© Stargate Inc. Tutti i diritti riservati.",
  "common.footerMeta": "Questo è un messaggio automatico. Non rispondere.",
  "common.fallbackLabel": "Il pulsante non funziona?",

  "signup-request.subject": "Conferma la tua email Stargate",
  "signup-request.preview":
    "Conferma il tuo indirizzo email per completare la creazione dell'account Stargate.",
  "signup-request.eyebrow": "Verifica account",
  "signup-request.title": "Conferma il tuo indirizzo email",
  "signup-request.lead":
    "Abbiamo ricevuto una richiesta di creazione di un account Stargate per questo indirizzo email.",
  "signup-request.body":
    "Conferma il tuo indirizzo per continuare. Il link qui sotto vale solo per questa richiesta e può essere usato una volta.",
  "signup-request.button": "Verifica l'indirizzo email",
  "signup-request.noteTitle": "Non hai richiesto questo?",
  "signup-request.note":
    "Nessun account è ancora stato creato. Se non aspettavi questa email, puoi ignorarla: non accadrà nulla.",

  "signup-account-exists.subject": "Hai già un account",
  "signup-account-exists.preview":
    "Un account usa già questo indirizzo email. Accedi per continuare.",
  "signup-account-exists.eyebrow": "Avviso account",
  "signup-account-exists.title": "Hai già un account",
  "signup-account-exists.lead":
    "Qualcuno ha richiesto un account Stargate con questo indirizzo email, ma esiste già un account, quindi non ne abbiamo creato uno nuovo.",
  "signup-account-exists.body":
    "Accedi qui sotto per continuare. Se hai dimenticato la password, scegli “Password dimenticata” nella pagina di accesso.",
  "signup-account-exists.button": "Accedi a Stargate",
  "signup-account-exists.noteTitle": "Il tuo account non è cambiato",
  "signup-account-exists.note":
    "Se non hai effettuato questa richiesta, non devi fare nulla. Le tue credenziali di accesso restano invariate.",

  "signup-completed.subject": "Benvenuto in Stargate!",
  "signup-completed.preview": "Il tuo account Stargate è pronto.",
  "signup-completed.eyebrow": "Benvenuto",
  "signup-completed.title": "Benvenuto in Stargate",
  "signup-completed.lead":
    "Il tuo account è stato creato correttamente. Ora puoi accedere con il tuo indirizzo email e la tua password.",
  "signup-completed.body": "Siamo felici di averti con noi.",
  "signup-completed.noteTitle": "Per iniziare",
  "signup-completed.note":
    "Accedi con il tuo indirizzo email e la tua password. Se la tua organizzazione richiede l'autenticazione a più fattori, ti guideremo al prossimo accesso.",

  "change-password-request.subject": "Richiesta di cambio password",
  "change-password-request.preview":
    "Scegli una nuova password per il tuo account Stargate.",
  "change-password-request.eyebrow": "Reimpostazione password",
  "change-password-request.title": "Reimposta la password",
  "change-password-request.lead":
    "Abbiamo ricevuto una richiesta di reimpostazione della password per il tuo account Stargate.",
  "change-password-request.body":
    "Scegli una nuova password con il pulsante qui sotto. Il link scade tra un'ora e può essere usato una volta.",
  "change-password-request.button": "Scegli una nuova password",
  "change-password-request.noteTitle": "Non hai richiesto questo?",
  "change-password-request.note":
    "La password non cambierà finché non usi il link qui sopra. Puoi ignorare questa email.",

  "password-changed.subject": "Password modificata",
  "password-changed.preview":
    "La password del tuo account Stargate è stata modificata.",
  "password-changed.eyebrow": "Sicurezza dell'account",
  "password-changed.title": "La tua password è stata modificata",
  "password-changed.lead":
    "La password del tuo account Stargate è stata modificata correttamente.",
  "password-changed.body": "Usa la nuova password al prossimo accesso.",
  "password-changed.noteTitle": "Non hai modificato la password?",
  "password-changed.note":
    "Reimposta subito la password. Se non riesci più ad accedere al tuo account, contatta l'amministratore della tua organizzazione.",

  "email-otp.subject": "Il tuo codice di verifica",
  "email-otp.preview": "Il tuo codice di verifica Stargate",
  "email-otp.eyebrow": "Codice di verifica",
  "email-otp.title": "Il tuo codice di verifica",
  "email-otp.lead":
    "Inserisci il codice qui sotto per accedere o completare la verifica. Può essere usato una volta.",
  "email-otp.expiry": "Il codice scade tra {{minutes}} min.",
  "email-otp.noteTitle": "Non condividere questo codice",
  "email-otp.note":
    "Stargate non ti chiederà mai di condividere questo codice. Se non l'hai richiesto, puoi ignorare questa email.",
};

const copies: Record<Locale, Copy> = { en, it };

export const subjectKeys: Record<TemplateId, CopyKey> = {
  "signup-request": "signup-request.subject",
  "signup-account-exists": "signup-account-exists.subject",
  "signup-completed": "signup-completed.subject",
  "change-password-request": "change-password-request.subject",
  "password-changed": "password-changed.subject",
  "email-otp": "email-otp.subject",
};

export function normalizeLocale(locale?: string | null): Locale {
  const primary = (locale ?? "").trim().toLowerCase().split(/[-_]/)[0];
  return primary === "it" ? "it" : "en";
}

export function t(locale: string | undefined, key: CopyKey): string {
  return copies[normalizeLocale(locale)][key];
}

export function buildSubjects(): Record<Locale, Record<TemplateId, string>> {
  const result = {} as Record<Locale, Record<TemplateId, string>>;
  for (const locale of locales) {
    const subjects = {} as Record<TemplateId, string>;
    for (const id of templateIds) {
      subjects[id] = t(locale, subjectKeys[id]);
    }
    result[locale] = subjects;
  }
  return result;
}
