//! Arabic translations, language selection and right-to-left layout helpers.
//!
//! English strings are the keys of the translation table. Every renderer passes its visible
//! text through [`t`] or [`rich`], so switching the language needs no per-page branch. The
//! active language is process-wide because egui text is built deep inside renderers that do not
//! receive the settings struct.

use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};

use eframe::egui::{self, Align, InnerResponse, Layout, RichText};
use serde::{Deserialize, Serialize};

/// The language chosen in Settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    /// Follow the operating system on startup.
    #[default]
    System,
    English,
    Arabic,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::System, Language::English, Language::Arabic];

    pub fn label(self) -> &'static str {
        match self {
            Language::System => t("Automatic"),
            Language::English => "English",
            Language::Arabic => "العربية",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Language::System => t("Use the language of this device."),
            Language::English => t("Show the interface in English."),
            Language::Arabic => t("Show the interface in Arabic, right to left."),
        }
    }

    /// Replace `System` with the language this device asks for.
    pub fn resolved(self) -> Language {
        match self {
            Language::System => match sys_locale::get_locale() {
                Some(locale) if locale.to_ascii_lowercase().starts_with("ar") => Language::Arabic,
                _ => Language::English,
            },
            other => other,
        }
    }
}

/// 0 = English, 1 = Arabic. Renderers read this on every frame.
static ACTIVE: AtomicU8 = AtomicU8::new(0);

pub fn set_language(language: Language) {
    let arabic = language.resolved() == Language::Arabic;
    ACTIVE.store(u8::from(arabic), Ordering::Relaxed);
}

pub fn is_arabic() -> bool {
    ACTIVE.load(Ordering::Relaxed) == 1
}

pub fn is_rtl() -> bool {
    is_arabic()
}

/// Translate a visible English string, returning it unchanged when no translation exists.
pub fn t(text: &str) -> &str {
    if is_rtl() {
        lookup(text).unwrap_or(text)
    } else {
        text
    }
}

/// Translate an owned string, for messages that were formatted at runtime.
pub fn t_owned(text: &str) -> String {
    if is_rtl() {
        message(text)
    } else {
        text.to_owned()
    }
}

/// Translate a runtime message, including the fixed part of messages that carry a value.
pub fn message(text: &str) -> String {
    if let Some(translated) = lookup(text) {
        return translated.to_owned();
    }
    for (prefix, arabic) in PREFIXES {
        if let Some(rest) = text.strip_prefix(prefix) {
            return format!("{arabic}{rest}");
        }
    }
    text.to_owned()
}

/// A translated rich-text label. Drop-in replacement for `RichText::new`.
pub fn rich(text: impl Into<String>) -> RichText {
    let text = text.into();
    if is_rtl() {
        RichText::new(t_owned(&text))
    } else {
        RichText::new(text)
    }
}

/// A horizontal row that mirrors in Arabic: items run right to left and text aligns right.
pub fn horizontal<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> InnerResponse<R> {
    let layout = Layout::left_to_right(Align::Center);
    let layout = if is_rtl() {
        Layout::right_to_left(Align::Center).with_main_wrap(false)
    } else {
        layout.with_main_wrap(false)
    };
    let size = egui::vec2(
        ui.available_size_before_wrap().x,
        ui.spacing().interact_size.y,
    );
    ui.allocate_ui_with_layout(size, layout, add)
}

/// A horizontal row aligned to the top of its tallest item.
pub fn horizontal_top<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> InnerResponse<R> {
    let layout = if is_rtl() {
        Layout::right_to_left(Align::TOP)
    } else {
        Layout::left_to_right(Align::TOP)
    };
    let size = ui.available_size_before_wrap();
    ui.allocate_ui_with_layout(size, layout, add)
}

/// A wrapping horizontal row of chips or pills.
pub fn horizontal_wrapped<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> InnerResponse<R> {
    let layout = if is_rtl() {
        Layout::right_to_left(Align::Center)
    } else {
        Layout::left_to_right(Align::Center)
    };
    let size = egui::vec2(
        ui.available_size_before_wrap().x,
        ui.spacing().interact_size.y,
    );
    ui.allocate_ui_with_layout(size, layout.with_main_wrap(true), add)
}

/// A vertical stack whose children align to the leading edge (right in Arabic).
pub fn vertical<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> InnerResponse<R> {
    let layout = Layout::top_down(if is_rtl() { Align::Max } else { Align::Min });
    ui.with_layout(layout, add)
}

/// The leading horizontal alignment for labels built outside a translated row.
pub fn align() -> Align {
    if is_rtl() { Align::Max } else { Align::Min }
}

/// The trailing horizontal alignment, where actions live.
pub fn trailing() -> Align {
    if is_rtl() { Align::Min } else { Align::Max }
}

/// A content row that mirrors in Arabic.
pub fn row_layout() -> Layout {
    if is_rtl() {
        Layout::right_to_left(Align::Center)
    } else {
        Layout::left_to_right(Align::Center)
    }
}

/// Translate a pattern that contains `{}` placeholders and fill them in order.
pub fn fill(pattern: &str, args: &[&dyn Display]) -> String {
    let template = if is_rtl() {
        lookup(pattern).unwrap_or(pattern)
    } else {
        pattern
    };
    let mut parts = template.split("{}");
    let mut out = parts.next().unwrap_or_default().to_owned();
    for (index, part) in parts.enumerate() {
        if let Some(arg) = args.get(index) {
            out.push_str(&arg.to_string());
        }
        out.push_str(part);
    }
    out
}

/// A layout that puts actions on the trailing edge: right in English, left in Arabic.
pub fn trailing_layout(valign: Align) -> Layout {
    if is_rtl() {
        Layout::left_to_right(valign)
    } else {
        Layout::right_to_left(valign)
    }
}

fn lookup(text: &str) -> Option<&'static str> {
    Some(match text {
        // Shell, pages, common controls.
        "Overview" => "نظرة عامة",
        "Servers" => "الخوادم",
        "History" => "السجل",
        "Settings" => "الإعدادات",
        "Catalogue" => "الدليل",
        "Latency" => "زمن الاستجابة",
        "Resolver" => "الخادم",
        "Score" => "النتيجة",
        "Progress" => "التقدم",
        "Ready" => "جاهز",
        "Running" => "قيد التشغيل",
        "WORKSPACE" => "مساحة العمل",
        "Connection overview" => "نظرة عامة على الاتصال",
        "Measure your connection and see what to use." => "قِس اتصالك واعرف ما الأنسب له.",
        "Find the fastest DNS resolver for your connection." => "اعثر على أسرع خادم DNS لاتصالك.",
        "Choose which public resolvers to test." => "اختر خوادم DNS العامة التي تريد اختبارها.",
        "Compare past runs and reopen a result." => "قارن الاختبارات السابقة وأعد فتح نتيجة.",
        "Appearance, benchmark defaults and data." => "المظهر وافتراضيات الاختبار والبيانات.",
        "Run benchmark" => "تشغيل الاختبار",
        "Run a benchmark" => "تشغيل اختبار",
        "Cancel" => "إلغاء",
        "Cancelling…" => "جارٍ الإلغاء…",
        "Save" => "حفظ",
        "Save settings" => "حفظ الإعدادات",
        "Reset settings" => "إعادة ضبط الإعدادات",
        "Refresh detection" => "تحديث الكشف",
        "Test all" => "اختبار الكل",
        "Test selected" => "اختبار المحدد",
        "Test favourites" => "اختبار المفضلة",
        "Test current" => "اختبار الحالي",
        "Select all" => "تحديد الكل",
        "Select none" => "إلغاء التحديد",
        "Apply DNS" => "تطبيق DNS",
        "Restore DNS" => "استعادة DNS",
        "Restore previous DNS" => "استعادة DNS السابق",
        "Restore previous DNS?" => "استعادة DNS السابق؟",
        "Cancel apply" => "إلغاء التطبيق",
        "Cancel revert" => "إلغاء الاستعادة",
        "Copy current DNS" => "نسخ DNS الحالي",
        "Copy endpoint" => "نسخ العنوان",
        "Copy setup value" => "نسخ قيمة الإعداد",
        "Close" => "إغلاق",
        "Close about" => "إغلاق حول",
        "Close details" => "إغلاق التفاصيل",
        "Close guide" => "إغلاق الدليل",
        "Delete" => "حذف",
        "Remove" => "إزالة",
        "Edit" => "تعديل",
        "Search" => "بحث",
        "Clear search" => "مسح البحث",
        "Retry" => "إعادة المحاولة",
        "Later" => "لاحقاً",
        "Download" => "تنزيل",
        "Open" => "فتح",
        "Yes" => "نعم",
        "No" => "لا",

        // Language.
        "System" => "النظام",
        "Automatic" => "تلقائي",
        "Language" => "اللغة",
        "Use the language of this device." => "استخدام لغة هذا الجهاز.",
        "Show the interface in English." => "عرض الواجهة بالإنجليزية.",
        "Show the interface in Arabic, right to left." => {
            "عرض الواجهة بالعربية، من اليمين إلى اليسار."
        }

        // Updates.
        "Updates" => "التحديثات",
        "Update" => "تحديث",
        "Check for updates" => "التحقق من التحديثات",
        "Checking for updates…" => "جارٍ التحقق من التحديثات…",
        "You are up to date." => "لديك أحدث إصدار.",
        "Update available" => "يتوفر تحديث",
        "Download and install" => "تنزيل وتثبيت",
        "Download update" => "تنزيل التحديث",
        "Downloading update…" => "جارٍ تنزيل التحديث…",
        "Verifying update…" => "جارٍ التحقق من التحديث…",
        "Installing update…" => "جارٍ تثبيت التحديث…",
        "Restart to finish" => "أعد التشغيل لإكمال التثبيت",
        "Update ready. dnsrace will restart." => "التحديث جاهز. ستعيد dnsrace التشغيل.",
        "Could not check for updates." => "تعذر التحقق من التحديثات.",
        "Could not download the update." => "تعذر تنزيل التحديث.",
        "Could not verify the update." => "تعذر التحقق من التحديث.",
        "Could not install the update." => "تعذر تثبيت التحديث.",
        "The downloaded file failed its checksum." => "فشل التحقق من بصمة الملف الذي تم تنزيله.",
        "Open the download page" => "فتح صفحة التنزيل",
        "Check automatically on startup" => "التحقق تلقائياً عند البدء",
        "Version" => "الإصدار",
        "Current version" => "الإصدار الحالي",
        "Latest version" => "أحدث إصدار",
        "Released" => "صدر في",
        "What's new" => "ما الجديد",
        "Skip this version" => "تخطي هذا الإصدار",
        "Remind me later" => "ذكّرني لاحقاً",
        "This update cannot install itself on Android. Download the APK and open it to update." => {
            "لا يمكن لهذا التحديث تثبيت نفسه على أندرويد. نزّل ملف APK وافتحه للتحديث."
        }

        // Settings.
        "Appearance" => "المظهر",
        "Theme" => "السمة",
        "Benchmark defaults" => "افتراضيات الاختبار",
        "Operating system" => "نظام التشغيل",
        "Interface" => "الواجهة",
        "Gateway" => "البوابة",
        "Saved data" => "البيانات المحفوظة",
        "{} ms · {} jobs · repeat {} · {} cached rounds · {} random lookups" => {
            "{} مللي ثانية · {} مهمة · تكرار {} · {} جولات مخزنة · {} استعلامات عشوائية"
        }
        "{} history · {} custom resolvers · {} favourites" => {
            "{} سجل · {} خوادم مخصصة · {} مفضلة"
        }
        "Benchmark setup" => "إعداد الاختبار",
        "Network and steering" => "الشبكة والتوجيه",
        "Data" => "البيانات",
        "Data management" => "إدارة البيانات",
        "Actions" => "الإجراءات",
        "Diagnostics" => "التشخيص",
        "About" => "حول",
        "About dnsrace" => "حول dnsrace",
        "Version {}" => "الإصدار {}",
        "Profile" => "الملف",
        "Protocol" => "البروتوكول",
        "Address family" => "عائلة العناوين",
        "Timeout" => "المهلة",
        "Jobs" => "المهام المتوازية",
        "Repeat" => "التكرار",
        "Cached rounds" => "الجولات المخزنة",
        "Random lookups" => "الاستعلامات العشوائية",
        "Include nearby open resolvers" => "تضمين الخوادم المفتوحة القريبة",
        "Include servers found on this machine" => "تضمين الخوادم الموجودة على هذا الجهاز",
        "Steering hostnames" => "أسماء التحقق من التوجيه",
        "One hostname per line" => "اسم مضيف واحد في كل سطر",
        "Export directory" => "مجلد التصدير",
        "Custom resolvers" => "خوادم مخصصة",
        "Saved runs" => "الاختبارات المحفوظة",
        "Keep history" => "الاحتفاظ بالسجل",
        "Clear history" => "مسح السجل",
        "Delete all runs" => "حذف كل الاختبارات",
        "Delete all history?" => "حذف كل السجل؟",
        "This removes every saved run from this device." => {
            "سيؤدي هذا إلى حذف كل اختبار محفوظ من هذا الجهاز."
        }
        "Saving writes these settings to this device. Reset restores the defaults." => {
            "الحفظ يكتب هذه الإعدادات على هذا الجهاز. إعادة الضبط تستعيد الافتراضيات."
        }
        "Export settings and data" => "تصدير الإعدادات والبيانات",
        "Import resolvers and favourites" => "استيراد الخوادم والمفضلة",
        "Leave the file blank to use the default export location." => {
            "اترك الملف فارغاً لاستخدام موقع التصدير الافتراضي."
        }
        "Path to a JSON file" => "مسار ملف JSON",
        "Export resolvers and favourites" => "تصدير الخوادم والمفضلة",
        "Import" => "استيراد",
        "Export" => "تصدير",
        "Copy diagnostics" => "نسخ التشخيص",
        "Diagnostics copied to the clipboard." => "تم نسخ التشخيص إلى الحافظة.",
        "Settings saved." => "تم حفظ الإعدادات.",
        "Settings reset to their defaults." => "تمت إعادة الإعدادات إلى افتراضياتها.",
        "MIT licence" => "رخصة MIT",
        "MIT licence · 100% Rust · No account, no telemetry." => {
            "رخصة MIT · 100% Rust · بلا حساب وبلا تتبع."
        }
        "Project website" => "موقع المشروع",
        "Report an issue" => "الإبلاغ عن مشكلة",
        "Open website" => "فتح الموقع",
        "Open source on GitHub" => "فتح المصدر على GitHub",
        "Made with Rust" => "صُنع بلغة Rust",

        // Profiles.
        "Quick" => "سريع",
        "Balanced" => "متوازن",
        "Thorough" => "شامل",
        "Custom" => "مخصص",
        "A short run for a first look." => "اختبار قصير لنظرة أولى.",
        "The recommended balance of speed and confidence." => {
            "التوازن الموصى به بين السرعة والثقة."
        }
        "More rounds and lookups for the most reliable result." => {
            "جولات واستعلامات أكثر لنتيجة أكثر موثوقية."
        }
        "Your own values. Changing a benchmark setting selects this profile." => {
            "قيمك الخاصة. تغيير أي إعداد للاختبار يختار هذا الملف."
        }
        "Editing any value switches the profile to Custom." => "تعديل أي قيمة يحوّل الملف إلى مخصص.",

        // Themes.
        "Dark" => "داكن",
        "Light" => "فاتح",
        "Nord" => "نورد",
        "Solarized" => "سولارايزد",
        "Dracula" => "دراكولا",
        "High contrast" => "تباين عالٍ",
        "The default Signal dark palette." => "لوحة Signal الداكنة الافتراضية.",
        "A bright palette for daylight." => "لوحة مشرقة للنهار.",
        "Cool blue-grey tones." => "درجات رمادية زرقاء هادئة.",
        "Warm, low-contrast readability." => "ألوان دافئة مريحة للقراءة.",
        "Dark purple with vivid accents." => "بنفسجي داكن مع لمسات زاهية.",
        "Maximum contrast for accessibility." => "أقصى تباين لإمكانية الوصول.",

        // Protocols and families.
        "Plain DNS" => "DNS عادي",
        "DNS-over-HTTPS" => "DNS عبر HTTPS",
        "DNS-over-TLS" => "DNS عبر TLS",
        "Any transport" => "أي نقل",
        "Any address" => "أي عنوان",
        "IPv4" => "IPv4",
        "IPv6" => "IPv6",
        "Both" => "كلاهما",

        // Server catalogue.
        "Resolver catalogue" => "دليل الخوادم",
        "All categories" => "كل الفئات",
        "Search provider, label or address" => "ابحث عن مزود أو اسم أو عنوان",
        "No resolvers match" => "لا توجد خوادم مطابقة",
        "Change the search or clear a filter to see the catalogue again." => {
            "غيّر البحث أو امسح عامل التصفية لرؤية الدليل مجدداً."
        }
        "Resolver details" => "تفاصيل الخادم",
        "Select a resolver from the table to see its endpoint, category and website." => {
            "اختر خادماً من الجدول لعرض عنوانه وفئته وموقعه."
        }
        "Favourites" => "المفضلة",
        "Add to favourites" => "إضافة إلى المفضلة",
        "Remove from favourites" => "إزالة من المفضلة",
        "Favourite" => "مفضل",
        "No favourite is available for the current protocol." => {
            "لا يوجد خادم مفضل متاح للبروتوكول الحالي."
        }
        "Star at least one resolver in Servers to enable Test favourites." => {
            "أضف نجمة إلى خادم واحد على الأقل في الخوادم لتفعيل اختبار المفضلة."
        }
        "Select at least one server for this protocol, or use Test all." => {
            "اختر خادماً واحداً على الأقل لهذا البروتوكول، أو استخدم اختبار الكل."
        }
        "Choose this resolver" => "اختر هذا الخادم",
        "Use this resolver" => "استخدم هذا الخادم",
        "Set up this resolver" => "إعداد هذا الخادم",
        "Endpoint" => "العنوان",
        "Category" => "الفئة",
        "Website" => "الموقع",
        "Addresses" => "العناوين",
        "Security" => "الأمان",
        "Speed" => "السرعة",
        "Name" => "الاسم",
        "Provider" => "المزود",
        "Public" => "عام",
        "Malware" => "برمجيات خبيثة",
        "Family" => "عائلي",

        // Benchmark page.
        "Network status" => "حالة الشبكة",
        "Current DNS" => "DNS الحالي",
        "Not detected yet" => "لم يُكشف بعد",
        "No DNS detected" => "لم يُكتشف DNS",
        "No current DNS servers are known yet. Use Refresh detection and try again." => {
            "لا تُعرف خوادم DNS الحالية بعد. استخدم تحديث الكشف وحاول مجدداً."
        }
        "Refresh detection to compare a future run with this network." => {
            "حدّث الكشف لمقارنة اختبار قادم بهذه الشبكة."
        }
        "Not tested" => "لم يُختبر",
        "Not measured" => "لم يُقس",
        "Not measured yet" => "لم يُقس بعد",
        "Measured with DNS-over-TLS in this run." => "قِيس باستخدام DNS عبر TLS في هذا الاختبار.",
        "From provider metadata. Run DNS-over-TLS to measure it directly." => {
            "من بيانات المزود. شغّل DNS عبر TLS لقياسه مباشرة."
        }
        "Results" => "النتائج",
        "Results ready" => "النتائج جاهزة",
        "Results appear here after a benchmark." => "تظهر النتائج هنا بعد الاختبار.",
        "Top results" => "أفضل النتائج",
        "Recommendation" => "التوصية",
        "No recommendation" => "لا توصية",
        "No recommendation yet" => "لا توصية بعد",
        "Run a benchmark and the best resolver will be explained here." => {
            "شغّل اختباراً وسيُشرح أفضل خادم هنا."
        }
        "Finish a benchmark and it will appear here with its winner." => {
            "أكمل اختباراً وسيظهر هنا مع أفضله."
        }
        "Fastest measured resolver" => "أسرع خادم مقيس",
        "Validates DNSSEC" => "يتحقق من DNSSEC",
        "Does not hijack NXDOMAIN" => "لا يعترض NXDOMAIN",
        "Stable latency" => "زمن استجابة مستقر",
        "Encrypted transport" => "نقل مشفر",
        "Best encrypted" => "أفضل مشفر",
        "Same as your current DNS" => "مطابق لـ DNS الحالي",
        "Choose {}" => "اختيار {}",
        "on {}" => "على {}",
        "via {}" => "عبر {}",
        "matched {}" => "مطابق لـ {}",
        "of {} scanned" => "من {} مفحوص",
        "Run details" => "تفاصيل الاختبار",
        "Duration" => "المدة",
        "Data center" => "مركز البيانات",
        "Scanned" => "تم فحصه",
        "Tested" => "تم اختباره",
        "ms" => "مللي ثانية",
        "Cached ms" => "مخزّنة",
        "Uncached ms" => "غير مخزّنة",
        "Cached" => "مخزّن",
        "Uncached" => "غير مخزّن",
        "Jitter" => "التذبذب",
        "Health" => "الحالة",
        "Unmeasured" => "غير مقاس",
        "Steering" => "التوجيه",
        "No steering data" => "لا بيانات توجيه",
        "Started" => "بدأ",
        "Finished" => "انتهى",

        // History.
        "No saved runs yet" => "لا اختبارات محفوظة بعد",
        "Select a saved run to reopen, export or delete it." => {
            "اختر اختباراً محفوظاً لإعادة فتحه أو تصديره أو حذفه."
        }
        "Reopen run" => "إعادة فتح الاختبار",
        "Delete run" => "حذف الاختبار",
        "Winner trend" => "اتجاه الفائز",
        "A trend appears after two runs with a measurable winner." => {
            "يظهر الاتجاه بعد اختبارين لهما فائز قابل للقياس."
        }
        "No run yet" => "لا اختبار بعد",
        "Last run" => "آخر اختبار",
        "History run reopened." => "أُعيد فتح اختبار من السجل.",
        "History entry deleted." => "حُذف إدخال من السجل.",
        "History cleared." => "تم مسح السجل.",

        // Reports and export.
        "Export JSON" => "تصدير JSON",
        "Export CSV" => "تصدير CSV",
        "Export HTML" => "تصدير HTML",
        "Share report" => "مشاركة التقرير",
        "Copy summary" => "نسخ الملخص",
        "Report copied to the clipboard." => "تم نسخ التقرير إلى الحافظة.",
        "Report shared." => "تمت مشاركة التقرير.",
        "Run a benchmark before exporting." => "شغّل اختباراً قبل التصدير.",
        "Run a benchmark before sharing a report." => "شغّل اختباراً قبل مشاركة تقرير.",
        "Exporting files is not available on Android. Share the report instead." => {
            "تصدير الملفات غير متاح على أندرويد. شارك التقرير بدلاً من ذلك."
        }
        "Android cannot write report files" => "لا يستطيع أندرويد كتابة ملفات التقارير",
        "dnsrace report" => "تقرير dnsrace",
        "dnsrace history report" => "تقرير سجل dnsrace",

        // Apply/revert and Android guide.
        "Change system DNS?" => "تغيير DNS النظام؟",
        "dnsrace will restore the settings saved when it applied:" => {
            "ستستعيد dnsrace الإعدادات المحفوظة عند التطبيق:"
        }
        "This changes the DNS servers for the connected network." => {
            "سيؤدي هذا إلى تغيير خوادم DNS للشبكة المتصلة."
        }
        "Apply now" => "تطبيق الآن",
        "Restoring the previous DNS settings…" => "جارٍ استعادة إعدادات DNS السابقة…",
        "Waiting for permission to change the system DNS settings…" => {
            "في انتظار الإذن لتغيير إعدادات DNS في النظام…"
        }
        "Previous DNS settings restored." => "تمت استعادة إعدادات DNS السابقة.",
        "No saved DNS change was found." => "لم يُعثر على تغيير DNS محفوظ.",
        "There is no saved DNS change to revert." => "لا يوجد تغيير DNS محفوظ لاستعادته.",
        "No current DNS servers are known yet." => "لا تُعرف خوادم DNS الحالية بعد.",
        "Private DNS settings" => "إعدادات DNS الخاصة",
        "Set Private DNS" => "تعيين DNS الخاصة",
        "Open Private DNS" => "فتح DNS الخاصة",
        "Wi-Fi settings" => "إعدادات Wi-Fi",
        "Open Wi-Fi" => "فتح Wi-Fi",
        "Open Wi-Fi settings." => "فتح إعدادات Wi-Fi.",
        "Set Wi-Fi DNS" => "تعيين DNS لـ Wi-Fi",
        "Change IP settings to Static." => "غيّر إعدادات IP إلى ثابت.",
        "Enter the resolver's IPv4 addresses in DNS 1/DNS 2." => {
            "أدخل عناوين IPv4 للخادم في DNS 1 وDNS 2."
        }
        "Choose Private DNS provider hostname and paste the hostname." => {
            "اختر اسم مضيف مزود DNS الخاصة وألصق اسم المضيف."
        }
        "Save and reconnect, then re-run dnsrace." => "احفظ وأعد الاتصال، ثم أعد تشغيل dnsrace.",
        "Save, then re-run dnsrace to confirm the result." => {
            "احفظ، ثم أعد تشغيل dnsrace لتأكيد النتيجة."
        }
        "Open Settings → Network & internet → Private DNS with a best-effort intent." => {
            "افتح الإعدادات ← الشبكة والإنترنت ← DNS الخاصة عبر نية النظام."
        }
        "Android 8 has no Private DNS screen" => "لا يوجد في أندرويد 8 شاشة DNS خاصة",
        "Android does not let dnsrace change DNS automatically. This guide only opens settings and copies values." => {
            "لا يسمح أندرويد لـ dnsrace بتغيير DNS تلقائياً. يفتح هذا الدليل الإعدادات وينسخ القيم فقط."
        }
        "Android does not let dnsrace change DNS. Use the setup guide instead." => {
            "لا يسمح أندرويد لـ dnsrace بتغيير DNS. استخدم دليل الإعداد بدلاً من ذلك."
        }
        "This is only available in the Android app." => "هذا متاح فقط في تطبيق أندرويد.",
        "Finish a benchmark before opening the setup guide." => "أكمل اختباراً قبل فتح دليل الإعداد.",
        "Finish a benchmark before applying a resolver." => "أكمل اختباراً قبل تطبيق خادم.",
        "Cloudflare (1.1.1.1)" => "Cloudflare (1.1.1.1)",
        "Cloudflare (malware)" => "Cloudflare (برمجيات خبيثة)",

        // Modals.
        "Add custom resolver" => "إضافة خادم مخصص",
        "Edit custom resolver" => "تعديل خادم مخصص",
        "Add resolver" => "إضافة خادم",
        "Cancel custom resolver" => "إلغاء الخادم المخصص",
        "Save resolver" => "حفظ الخادم",
        "Optional label" => "اسم اختياري",
        "DoH URL" => "رابط DoH",
        "IP address (optional)" => "عنوان IP (اختياري)",
        "Remove {}" => "إزالة {}",
        "Edit {}" => "تعديل {}",
        "Remove Example" => "إزالة مثال",
        "Edit Example" => "تعديل مثال",
        "No custom resolvers yet." => "لا خوادم مخصصة بعد.",
        "That custom resolver no longer exists." => "هذا الخادم المخصص لم يعد موجوداً.",
        "Custom resolver removed." => "أُزيل الخادم المخصص.",

        // Status and security.
        "Secure" => "آمن",
        "Insecure" => "غير آمن",
        "Validated" => "مُتحقق",
        "Partial" => "جزئي",
        "Bogus" => "مزيف",
        "Unknown" => "غير معروف",
        "No DNSSEC" => "بلا DNSSEC",
        "DNSSEC partial" => "DNSSEC جزئي",
        "DNSSEC unknown" => "DNSSEC غير معروف",
        "Bogus accepted" => "يقبل المزيف",
        "No hijack" => "لا اعتراض",
        "Hijacks NXDOMAIN" => "يعترض NXDOMAIN",
        "Not checked" => "لم يُفحص",

        // Common benchmark states.
        "Preparing the benchmark" => "تحضير الاختبار",
        "Checking which servers answer" => "التحقق من الخوادم المستجيبة",
        "Measuring latency" => "قياس زمن الاستجابة",
        "Testing steering" => "اختبار التوجيه",
        "Done" => "تم",
        "Scan" => "فحص",
        "Test" => "اختبار",
        "No usable row" => "لا صف قابل للاستخدام",
        "Finished, but no resolver completed enough measurements." => {
            "انتهى، لكن لم يكمل أي خادم قياسات كافية."
        }
        "Benchmark cancelled. Partial results are shown." => "أُلغي الاختبار. تُعرض النتائج الجزئية.",
        "The benchmark ended unexpectedly. Try again." => {
            "انتهى الاختبار بشكل غير متوقع. حاول مجدداً."
        }
        "DNS detection ended unexpectedly." => "انتهى كشف DNS بشكل غير متوقع.",
        "The apply operation ended unexpectedly." => "انتهت عملية التطبيق بشكل غير متوقع.",
        "The revert operation ended unexpectedly." => "انتهت عملية الاستعادة بشكل غير متوقع.",
        "The apply request is no longer available." => "طلب التطبيق لم يعد متاحاً.",
        "No usable address was found for that resolver." => "لم يُعثر على عنوان صالح لهذا الخادم.",
        "That provider has no website in the catalogue." => "لا يوجد موقع لهذا المزود في الدليل.",
        "That resolver has no endpoint." => "لا يوجد عنوان لهذا الخادم.",
        "That history entry no longer exists." => "إدخال السجل هذا لم يعد موجوداً.",
        "There is no hostname or address to copy yet." => "لا يوجد اسم مضيف أو عنوان لنسخه بعد.",
        "No value to copy yet" => "لا قيمة لنسخها بعد",
        "This run did not produce a usable row." => "لم ينتج هذا الاختبار صفاً قابلاً للاستخدام.",
        "Not enough measurements" => "قياسات غير كافية",

        // Diagnostics and import/export.
        "Update state" => "حالة التحديث",
        "Configuration folder" => "مجلد الإعدادات",
        "Imported {} resolvers and {} favourites." => "تم استيراد {} خادماً و{} مفضلة.",
        "{} selected · {} available" => "{} محدد · {} متاح",
        "{} shown" => "{} ظاهر",
        "By {}" => "بواسطة {}",
        "{} measured" => "{} مقيس",
        "{} measured run(s)" => "{} اختبار مقيس",
        "{} run(s)" => "{} اختبار",
        "{} saved benchmark run(s)" => "{} اختبار محفوظ",
        "Profile: {}" => "الملف: {}",
        "Profile: {} · {} cached rounds · {} random lookups" => {
            "الملف: {} · {} جولات مخزنة · {} استعلامات عشوائية"
        }
        "Current DNS: {}" => "DNS الحالي: {}",
        "Data center: {}" => "مركز البيانات: {}",
        "{} timeout(s) · {}" => "{} مهلة · {}",
        "dnsrace will use {}" => "ستستخدم dnsrace {}",
        "Current version {}" => "الإصدار الحالي {}",
        "The top two are within {} ms; either is a good choice." => {
            "أفضل نتيجتين ضمن {} مللي ثانية؛ كلتاهما خيار جيد."
        }
        "Fastest applicable resolver: {} ({})" => "أسرع خادم قابل للتطبيق: {} ({})",
        "Exported resolvers and favourites." => "تم تصدير الخوادم والمفضلة.",
        "Could not import that file." => "تعذر استيراد هذا الملف.",
        "That file is not a dnsrace resolver export." => "هذا الملف ليس تصدير خوادم dnsrace.",
        "Resolvers" => "الخوادم",
        "Imported" => "مستورد",
        "Invalid" => "غير صالح",

        // Update messages.
        "an update is available" => "يتوفر تحديث",
        "checking" => "جارٍ التحقق",
        "downloading" => "جارٍ التنزيل",
        "verifying" => "جارٍ التحقق",
        "installing" => "جارٍ التثبيت",
        "restarting" => "إعادة التشغيل",
        "failed" => "فشل",
        "not checked" => "لم يُتحقق",
        "up to date" => "محدّث",
        "version {} available" => "الإصدار {} متوفر",
        "no update is available" => "لا يتوفر تحديث",
        "the last check failed" => "فشل آخر تحقق",
        "never checked" => "لم يُتحقق بعد",
        "General" => "عام",
        "Privacy" => "خصوصية",
        "Uncensored" => "بلا رقابة",
        "Standard DNS on port 53, the default" => "DNS القياسي على المنفذ 53، الافتراضي",
        "DNS over TCP, for networks that block UDP port 53" => {
            "DNS عبر TCP للشبكات التي تحجب منفذ UDP 53"
        }
        "Encrypted DNS over TLS on port 853" => "DNS مشفر عبر TLS على المنفذ 853",
        "Encrypted DNS over HTTPS (RFC 8484)" => "DNS مشفر عبر HTTPS (RFC 8484)",
        "IPv4 + IPv6" => "IPv4 + IPv6",
        "OK" => "سليم",
        "Unreliable" => "غير موثوق",
        "DNSSEC validated" => "DNSSEC مُتحقق",
        "Accepts bogus DNSSEC" => "يقبل DNSSEC المزيف",
        "DNSSEC ✓" => "DNSSEC ✓",
        "DNSSEC ?" => "DNSSEC ؟",
        "Measuring latency and DNSSEC" => "قياس زمن الاستجابة وDNSSEC",
        "Checking where hostnames are steered" => "التحقق من وجهة توجيه أسماء المضيف",
        "A fast pass with fewer rounds. Good for a first look." => {
            "مرور سريع بجولات أقل. مناسب لنظرة أولى."
        }
        "The default balance of speed and confidence." => "التوازن الافتراضي بين السرعة والثقة.",
        "More rounds and random names for a steadier ranking." => {
            "جولات وأسماء عشوائية أكثر لترتيب أكثر ثباتاً."
        }
        "Your own timeout, concurrency and depth values." => "قيمك الخاصة للمهلة والتوازي والعمق.",
        "Midnight" => "منتصف الليل",
        "Solarized Light" => "سولارايزد فاتح",
        "Follow the operating system's light or dark setting." => {
            "اتّباع إعداد الفاتح أو الداكن في نظام التشغيل."
        }
        "The default neutral dark palette." => "اللوحة الداكنة المحايدة الافتراضية.",
        "A clean light palette for bright rooms." => "لوحة فاتحة نظيفة للغرف المضيئة.",
        "Deep navy with cool blue highlights." => "كحلي عميق مع لمسات زرقاء باردة.",
        "Muted arctic blue-grey, easy on the eyes." => "رمادي أزرق قطبي هادئ ومريح للعين.",
        "Warm, low-contrast light palette." => "لوحة فاتحة دافئة منخفضة التباين.",

        // Remaining visible strings and dynamic templates.
        "Benchmark history" => "سجل الاختبارات",
        "Settings actions" => "إجراءات الإعدادات",
        "DNS addresses" => "عناوين DNS",
        "Benchmark DoT servers instead" => "اختبر خوادم DoT بدلاً من ذلك",
        "Benchmark plain DNS instead" => "اختبر DNS العادي بدلاً من ذلك",
        "Details close with Escape or the close button." => {
            "تُغلق التفاصيل بمفتاح Escape أو زر الإغلاق."
        }
        "The last run's duration, depth and data center appear here." => {
            "تظهر هنا مدة آخر اختبار وعمقه ومركز البيانات."
        }
        "Stack trace:" => "تتبّع المكدس:",
        "Current DNS is not known yet." => "DNS الحالي غير معروف بعد.",
        "Check for updates first." => "تحقق من التحديثات أولاً.",
        "Applied via {}" => "طُبّق عبر {}",
        "Finished: {} is fastest at {} ms." => "انتهى: {} الأسرع عند {} مللي ثانية.",
        "Refreshed: {} DNS server(s) detected." => "تم التحديث: اكتُشف {} خادم DNS.",
        "Refreshed: no system DNS servers were detected." => {
            "تم التحديث: لم يُكتشف أي خادم DNS في النظام."
        }
        "{} ms timeout · {} jobs · {} repeat · {} cached rounds · {} random lookups" => {
            "{} مللي ثانية مهلة · {} مهمة · {} تكرار · {} جولات مخزنة · {} استعلامات عشوائية"
        }
        "{} of {}" => "{} من {}",
        "{} ms · {}" => "{} مللي ثانية · {}",
        " · {} cached rounds · {} random lookups" => " · {} جولات مخزنة · {} استعلامات عشوائية",
        "{} ms faster than your current DNS{}" => "أسرع بـ {} مللي ثانية من DNS الحالي{}",
        "{} ms slower than your current DNS{}" => "أبطأ بـ {} مللي ثانية من DNS الحالي{}",
        "Export {}" => "تصدير {}",
        "Open run {}" => "فتح الاختبار {}",
        "Select {} {}" => "تحديد {} {}",
        "Favourite {} {}" => "تفضيل {} {}",
        "Open {} {}" => "فتح {} {}",
        _ => return None,
    })
}

/// Fixed beginnings of runtime messages that carry a value after a colon or a name.
const PREFIXES: &[(&str, &str)] = &[
    ("Could not save settings: ", "تعذر حفظ الإعدادات: "),
    ("Could not save history: ", "تعذر حفظ السجل: "),
    ("Could not clear history: ", "تعذر مسح السجل: "),
    ("Could not save the report: ", "تعذر حفظ التقرير: "),
    ("Could not share the report: ", "تعذر مشاركة التقرير: "),
    ("Could not export: ", "تعذر التصدير: "),
    ("Could not copy: ", "تعذر النسخ: "),
    ("Could not open the link: ", "تعذر فتح الرابط: "),
    ("Could not open ", "تعذر فتح "),
    ("Could not apply DNS: ", "تعذر تطبيق DNS: "),
    ("Could not restore DNS: ", "تعذر استعادة DNS: "),
    (
        "Could not start the apply operation: ",
        "تعذر بدء عملية التطبيق: ",
    ),
    (
        "Could not start the revert operation: ",
        "تعذر بدء عملية الاستعادة: ",
    ),
    (
        "Could not start the benchmark runtime: ",
        "تعذر بدء بيئة الاختبار: ",
    ),
    (
        "Could not start the benchmark thread: ",
        "تعذر بدء خيط الاختبار: ",
    ),
    (
        "DNS was restored, but state cleanup failed: ",
        "تمت استعادة DNS، لكن فشل تنظيف الحالة: ",
    ),
    ("Saved ", "حُفظ في "),
    ("Added ", "أُضيف "),
    ("Updated ", "حُدّث "),
    ("Applied ", "طُبّق "),
    ("Opened ", "فُتح "),
    ("Copied ", "نُسخ "),
    ("Finished: ", "انتهى: "),
    (
        "DNS was applied, but the revert information could not be saved: ",
        "طُبّق DNS، لكن تعذر حفظ معلومات الاستعادة: ",
    ),
    ("Refreshed: ", "تم التحديث: "),
    ("Could not update: ", "تعذر التحديث: "),
    (
        "Could not check for updates: ",
        "تعذر التحقق من التحديثات: ",
    ),
    ("Could not import that file: ", "تعذر استيراد هذا الملف: "),
    ("Current DNS: ", "DNS الحالي: "),
    ("Version ", "الإصدار "),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_a_label() {
        for language in Language::ALL {
            assert!(!language.label().is_empty());
        }
    }

    #[test]
    fn arabic_translation_is_available_for_common_labels() {
        assert_eq!(
            translate_for(Language::Arabic, "Settings"),
            Some("الإعدادات")
        );
        assert_eq!(
            translate_for(Language::Arabic, "Run benchmark"),
            Some("تشغيل الاختبار")
        );
        assert!(translate_for(Language::Arabic, "not a real key").is_none());
    }

    #[test]
    fn system_language_resolves_to_english_or_arabic() {
        assert!(matches!(
            Language::System.resolved(),
            Language::English | Language::Arabic
        ));
    }

    #[test]
    fn dynamic_messages_keep_their_value() {
        assert_eq!(message("Saved /tmp/report.json"), "حُفظ في /tmp/report.json");
    }

    fn translate_for(language: Language, text: &str) -> Option<&'static str> {
        if language == Language::Arabic {
            lookup(text)
        } else {
            None
        }
    }
}
