//! Localized transactional mail with a shared plain-text and HTML presentation.

use crate::i18n::language_preferences;

pub(super) enum MailKind<'a> {
    LoginCode(&'a str),
    ResetCode(&'a str),
    AdminResetToken(&'a str),
    SmtpTest,
}

pub(super) struct MailMessage {
    pub(super) subject: String,
    pub(super) text: String,
    pub(super) html: String,
}

struct Copy {
    title: &'static str,
    lead: &'static str,
    note: &'static str,
}

enum Language {
    Chinese,
    Russian,
    English,
}

impl Language {
    fn from_locale(locale: &str) -> Self {
        for preference in language_preferences(locale) {
            let language = preference.split(['-', '_']).next().unwrap_or_default();
            if language.eq_ignore_ascii_case("zh") {
                return Self::Chinese;
            }
            if language.eq_ignore_ascii_case("ru") {
                return Self::Russian;
            }
            if language.eq_ignore_ascii_case("en") {
                return Self::English;
            }
        }
        Self::English
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Chinese => "zh-CN",
            Self::Russian => "ru",
            Self::English => "en",
        }
    }

    fn copy(&self, kind: &MailKind<'_>) -> Copy {
        match (self, kind) {
            (Self::Chinese, MailKind::LoginCode(_)) => Copy {
                title: "登录验证码",
                lead: "在 RustDesk Console 登录页面输入以下验证码。",
                note: "验证码在 10 分钟后失效。若不是你发起的登录，请忽略此邮件。",
            },
            (Self::Chinese, MailKind::ResetCode(_)) => Copy {
                title: "密码重置验证码",
                lead: "在 RustDesk Console 的密码重置页面输入以下验证码。",
                note: "验证码在 15 分钟后失效。若你没有申请重置，请忽略此邮件。",
            },
            (Self::Chinese, MailKind::AdminResetToken(_)) => Copy {
                title: "管理员发起了密码重置",
                lead: "在 RustDesk Console 登录页选择「找回密码」→「使用邮件重置口令」，粘贴完整口令并设置新密码。",
                note: "重置口令仅可使用一次，15 分钟后失效。若你不需要重置，请忽略此邮件。",
            },
            (Self::Chinese, MailKind::SmtpTest) => Copy {
                title: "SMTP 测试邮件",
                lead: "RustDesk Console 已使用当前 SMTP 配置成功发出此邮件。",
                note: "",
            },
            (Self::Russian, MailKind::LoginCode(_)) => Copy {
                title: "Код для входа",
                lead: "Введите этот код на странице входа в RustDesk Console.",
                note: "Код действителен 10 минут. Если вы не пытались войти, проигнорируйте это письмо.",
            },
            (Self::Russian, MailKind::ResetCode(_)) => Copy {
                title: "Код для сброса пароля",
                lead: "Введите этот код на странице сброса пароля RustDesk Console.",
                note: "Код действителен 15 минут. Если вы не запрашивали сброс, проигнорируйте это письмо.",
            },
            (Self::Russian, MailKind::AdminResetToken(_)) => Copy {
                title: "Администратор запросил сброс пароля",
                lead: "На странице входа RustDesk Console выберите восстановление пароля, затем использование кода из письма. Вставьте полный код и задайте новый пароль.",
                note: "Одноразовый код действителен 15 минут. Если сброс не нужен, проигнорируйте это письмо.",
            },
            (Self::Russian, MailKind::SmtpTest) => Copy {
                title: "Проверка SMTP",
                lead: "RustDesk Console отправила это письмо с текущими настройками SMTP.",
                note: "",
            },
            (Self::English, MailKind::LoginCode(_)) => Copy {
                title: "Sign-in verification code",
                lead: "Enter this code on the RustDesk Console sign-in page.",
                note: "The code expires in 10 minutes. If you did not try to sign in, ignore this email.",
            },
            (Self::English, MailKind::ResetCode(_)) => Copy {
                title: "Password reset code",
                lead: "Enter this code on the RustDesk Console password reset page.",
                note: "The code expires in 15 minutes. If you did not request a reset, ignore this email.",
            },
            (Self::English, MailKind::AdminResetToken(_)) => Copy {
                title: "An administrator requested a password reset",
                lead: "On the RustDesk Console sign-in page, choose Forgot password, then Use an emailed reset token. Paste the complete token and choose a new password.",
                note: "This token can be used once and expires in 15 minutes. If you do not need a reset, ignore this email.",
            },
            (Self::English, MailKind::SmtpTest) => Copy {
                title: "SMTP test email",
                lead: "RustDesk Console sent this email using the current SMTP settings.",
                note: "",
            },
        }
    }
}

pub(super) fn compose(locale: &str, kind: MailKind<'_>) -> MailMessage {
    let language = Language::from_locale(locale);
    let copy = language.copy(&kind);
    let code = match kind {
        MailKind::LoginCode(code) | MailKind::ResetCode(code) | MailKind::AdminResetToken(code) => {
            Some(code)
        }
        MailKind::SmtpTest => None,
    };
    let code_html = code.map_or_else(String::new, |code| {
        format!(
            r#"<p style="margin:24px 0;padding:14px;border:1px solid #dce4f3;border-radius:6px;background:#f1f5fd;color:#163569;font:700 20px/1.4 monospace;word-break:break-all">{}</p>"#,
            escape(code)
        )
    });
    let note_html = if copy.note.is_empty() {
        String::new()
    } else {
        format!(
            r#"<p style="margin:18px 0 0;color:#475569;font-size:14px;line-height:1.55">{}</p>"#,
            escape(copy.note)
        )
    };
    let html = format!(
        r#"<!doctype html>
<html lang="{}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"></head>
<body style="margin:0;background:#f3f6fb;font:16px/1.55 Arial,sans-serif;color:#17233b">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0"><tr><td align="center" style="padding:24px 12px">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:560px;background:#ffffff;border:1px solid #dce4f3">
<tr><td style="background:#162e55;color:#ffffff;padding:13px 22px;font-weight:bold">RustDesk Console</td></tr>
<tr><td style="padding:22px"><h1 style="font-size:23px;line-height:1.3;margin:0 0 12px">{}</h1>
<p style="margin:0">{}</p>{}{}</td></tr></table>
</td></tr></table></body></html>"#,
        language.code(),
        escape(copy.title),
        escape(copy.lead),
        code_html,
        note_html,
    );
    let text = [Some(copy.title), Some(copy.lead), code, Some(copy.note)]
        .into_iter()
        .flatten()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    MailMessage {
        subject: format!("RustDesk Console · {}", copy.title),
        text,
        html,
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_localize_and_escape_credentials() {
        for (locale, heading) in [
            ("en;q=0.1, zh-CN;q=0.9", "管理员发起了密码重置"),
            ("fr, RU-RU;q=0.9", "Администратор запросил сброс пароля"),
            ("zh;q=0, en", "An administrator requested a password reset"),
        ] {
            let message = compose(locale, MailKind::AdminResetToken("abc.<123456>"));
            assert!(message.subject.contains(heading));
            assert!(message.text.contains("abc.<123456>"));
            assert!(message.html.contains("abc.&lt;123456&gt;"));
            assert!(!message.html.contains("abc.<123456>"));
        }
        assert!(!compose("zh", MailKind::SmtpTest)
            .html
            .contains("word-break"));
    }

    #[test]
    fn templates_cover_every_transactional_email() {
        for (locale, expected) in [
            ("zh_CN", ["登录验证码", "密码重置验证码", "SMTP 测试邮件"]),
            (
                "ru",
                ["Код для входа", "Код для сброса пароля", "Проверка SMTP"],
            ),
            (
                "en",
                [
                    "Sign-in verification code",
                    "Password reset code",
                    "SMTP test email",
                ],
            ),
        ] {
            for (kind, title) in [
                MailKind::LoginCode("123456"),
                MailKind::ResetCode("123456"),
                MailKind::SmtpTest,
            ]
            .into_iter()
            .zip(expected)
            {
                let message = compose(locale, kind);
                assert!(message.subject.ends_with(title));
                assert!(message.text.starts_with(title));
                assert!(message.html.contains(title));
            }
        }
    }
}
