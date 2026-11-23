//! Built-in word lists. Combinations are synthetic; no list contains real
//! personal records. Email domains are reserved for testing (RFC 2606), and
//! English phone numbers use the unassigned `555` area code.

use workbench_domain::Locale;

pub(crate) struct Words {
    pub first_names: &'static [&'static str],
    pub last_names: &'static [&'static str],
    pub streets: &'static [&'static str],
    pub cities: &'static [&'static str],
    pub company_a: &'static [&'static str],
    pub company_b: &'static [&'static str],
}

pub(crate) const EMAIL_DOMAINS: &[&str] = &["example.test", "example.com", "example.org"];

pub(crate) const STREET_SUFFIXES: &[&str] = &["St", "Ave", "Rd", "Ln", "Blvd", "Way"];
pub(crate) const COMPANY_SUFFIXES: &[&str] = &["Inc", "LLC", "Co", "Ltd"];

pub(crate) const PLAIN_WORDS: &[&str] = &[
    "amber", "bright", "canyon", "delta", "ember", "falcon", "garden", "harbor", "island",
    "jasper", "kernel", "lantern", "meadow", "nimbus", "orbit", "pepper", "quartz", "river",
    "summit", "timber", "umbra", "velvet", "willow", "xenon", "yonder", "zephyr", "anchor",
    "breeze", "cobalt", "dune",
];

const EN: Words = Words {
    first_names: &[
        "Alex", "Jordan", "Taylor", "Morgan", "Casey", "Riley", "Jamie", "Avery", "Quinn",
        "Harper", "Drew", "Rowan", "Sam", "Charlie", "Emerson", "Skyler", "Reese", "Parker",
        "Logan", "Hayden",
    ],
    last_names: &[
        "Morgan", "Carter", "Bennett", "Brooks", "Hayes", "Foster", "Reed", "Sullivan", "Wallace",
        "Griffin", "Porter", "Hughes", "Ellis", "Coleman", "Fisher", "Graham", "Hart", "Palmer",
        "Shaw", "Watts",
    ],
    streets: &[
        "Oak", "Maple", "Cedar", "Pine", "Elm", "Willow", "Lake", "Hill", "Park", "River",
        "Sunset", "Meadow",
    ],
    cities: &[
        "Springfield",
        "Riverton",
        "Fairview",
        "Lakeside",
        "Greenville",
        "Oakridge",
        "Brookfield",
        "Ashford",
        "Millbrook",
        "Westfield",
    ],
    company_a: &[
        "Blue", "Bright", "North", "Summit", "Pioneer", "Crescent", "Atlas", "Harbor", "Vertex",
        "Cobalt", "Silver", "Granite",
    ],
    company_b: &[
        "Labs",
        "Systems",
        "Works",
        "Dynamics",
        "Partners",
        "Logistics",
        "Foods",
        "Media",
    ],
};

const ID: Words = Words {
    first_names: &[
        "Budi", "Siti", "Agus", "Dewi", "Rizky", "Putri", "Andi", "Ayu", "Eko", "Sri", "Bayu",
        "Indah", "Fajar", "Nur", "Dimas", "Rina", "Hendra", "Wulan", "Yusuf", "Maya",
    ],
    last_names: &[
        "Santoso",
        "Wijaya",
        "Pratama",
        "Saputra",
        "Lestari",
        "Hidayat",
        "Kusuma",
        "Nugroho",
        "Setiawan",
        "Wulandari",
        "Siregar",
        "Nasution",
        "Hutapea",
        "Simanjuntak",
        "Gunawan",
        "Rahayu",
        "Susanto",
        "Halim",
        "Syahputra",
        "Permana",
    ],
    streets: &[
        "Sudirman",
        "Thamrin",
        "Gatot Subroto",
        "Diponegoro",
        "Ahmad Yani",
        "Merdeka",
        "Pemuda",
        "Asia Afrika",
        "Pahlawan",
        "Kartini",
        "Veteran",
        "Cendana",
    ],
    cities: &[
        "Jakarta",
        "Surabaya",
        "Bandung",
        "Medan",
        "Semarang",
        "Makassar",
        "Yogyakarta",
        "Denpasar",
        "Palembang",
        "Malang",
        "Balikpapan",
        "Pontianak",
    ],
    company_a: &[
        "Maju",
        "Sinar",
        "Karya",
        "Nusantara",
        "Cahaya",
        "Mitra",
        "Bumi",
        "Surya",
        "Tunas",
        "Mega",
        "Lintas",
        "Sentra",
    ],
    company_b: &[
        "Jaya",
        "Abadi",
        "Sejahtera",
        "Makmur",
        "Mandiri",
        "Sentosa",
        "Bersama",
        "Utama",
    ],
};

pub(crate) const fn words(locale: Locale) -> &'static Words {
    match locale {
        Locale::En => &EN,
        Locale::Id => &ID,
    }
}
