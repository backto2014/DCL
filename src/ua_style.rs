
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Display {
    Block,
    Inline,
    InlineBlock,
    ListItem,
    Table,
    TableRow,
    TableCell,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cor(pub u8, pub u8, pub u8);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lados {
    pub topo: f32,
    pub dir: f32,
    pub base: f32,
    pub esq: f32,
}

impl Lados {
    pub const ZERO: Lados = Lados { topo: 0.0, dir: 0.0, base: 0.0, esq: 0.0 };

    pub const fn vertical(v: f32) -> Lados {
        Lados { topo: v, dir: 0.0, base: v, esq: 0.0 }
    }
    pub const fn horizontal(h: f32) -> Lados {
        Lados { topo: 0.0, dir: h, base: 0.0, esq: h }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EstiloPadrao {
    pub display: Display,
    pub fonte_escala: f32,
    pub negrito: bool,
    pub italico: bool,
    pub sublinhado: bool,
    pub tachado: bool,
    pub monoespaco: bool,
    pub preformatado: bool,
    pub cor: Option<Cor>,
    pub cor_fundo: Option<Cor>,
    pub margem: Lados,
    pub padding: Lados,
}

impl Default for EstiloPadrao {
    fn default() -> Self {
        EstiloPadrao {
            display: Display::Inline,
            fonte_escala: 1.0,
            negrito: false,
            italico: false,
            sublinhado: false,
            tachado: false,
            monoespaco: false,
            preformatado: false,
            cor: None,
            cor_fundo: None,
            margem: Lados::ZERO,
            padding: Lados::ZERO,
        }
    }
}

fn bloco() -> EstiloPadrao {
    EstiloPadrao { display: Display::Block, ..Default::default() }
}

fn titulo(escala: f32, margem_v: f32) -> EstiloPadrao {
    EstiloPadrao {
        display: Display::Block,
        fonte_escala: escala,
        negrito: true,
        margem: Lados::vertical(margem_v),
        ..Default::default()
    }
}

pub fn estilo_padrao(tag: &str) -> EstiloPadrao {
    use Display::*;

    match tag {
        "head" | "script" | "style" | "title" | "meta" | "link" | "base"
        | "template" | "noscript" | "area" | "datalist" | "param"
        | "source" | "track" | "dialog" => EstiloPadrao { display: None, ..Default::default() },

        "html" => bloco(),
        "body" => EstiloPadrao { margem: Lados { topo: 0.5, dir: 0.5, base: 0.5, esq: 0.5 }, ..bloco() },

        "div" | "section" | "article" | "aside" | "nav" | "header"
        | "footer" | "main" | "figure" | "form" | "fieldset" | "details"
        | "summary" | "address" | "hgroup" | "search" => bloco(),

        "h1" => titulo(2.0, 0.67),
        "h2" => titulo(1.5, 0.83),
        "h3" => titulo(1.17, 1.0),
        "h4" => titulo(1.0, 1.33),
        "h5" => titulo(0.83, 1.67),
        "h6" => titulo(0.67, 2.33),

        "p" | "dl" => EstiloPadrao { margem: Lados::vertical(1.0), ..bloco() },
        "blockquote" => EstiloPadrao {
            margem: Lados { topo: 1.0, dir: 2.5, base: 1.0, esq: 2.5 }, // 40px
            ..bloco()
        },
        "pre" | "listing" | "plaintext" | "xmp" => EstiloPadrao {
            monoespaco: true,
            preformatado: true,
            margem: Lados::vertical(1.0),
            ..bloco()
        },
        "hr" => EstiloPadrao {
            margem: Lados::vertical(0.5),
            cor_fundo: Some(Cor(128, 128, 128)), // aproxima a linha cinza
            ..bloco()
        },
        "center" => bloco(),
        "dd" => EstiloPadrao { margem: Lados { topo: 0.0, dir: 0.0, base: 0.0, esq: 2.5 }, ..bloco() },
        "dt" => bloco(),

        "ul" | "ol" | "menu" | "dir" => EstiloPadrao {
            margem: Lados::vertical(1.0),
            padding: Lados { topo: 0.0, dir: 0.0, base: 0.0, esq: 2.5 }, // 40px
            ..bloco()
        },
        "li" => EstiloPadrao { display: ListItem, ..Default::default() },

        "table" => EstiloPadrao { display: Table, ..Default::default() },
        "tr" => EstiloPadrao { display: TableRow, ..Default::default() },
        "td" => EstiloPadrao {
            display: TableCell,
            padding: Lados { topo: 0.1, dir: 0.1, base: 0.1, esq: 0.1 },
            ..Default::default()
        },
        "th" => EstiloPadrao {
            display: TableCell,
            negrito: true,
            padding: Lados { topo: 0.1, dir: 0.1, base: 0.1, esq: 0.1 },
            ..Default::default()
        },
        "thead" | "tbody" | "tfoot" => EstiloPadrao { display: Block, ..Default::default() },
        "caption" => bloco(),

        "b" | "strong" => EstiloPadrao { negrito: true, ..Default::default() },
        "i" | "em" | "cite" | "dfn" | "var" => EstiloPadrao { italico: true, ..Default::default() },
        "u" | "ins" => EstiloPadrao { sublinhado: true, ..Default::default() },
        "s" | "strike" | "del" => EstiloPadrao { tachado: true, ..Default::default() },
        "mark" => EstiloPadrao {
            cor: Some(Cor(0, 0, 0)),
            cor_fundo: Some(Cor(255, 255, 0)),
            ..Default::default()
        },

        "small" | "sub" | "sup" => EstiloPadrao { fonte_escala: 0.83, ..Default::default() },
        "big" => EstiloPadrao { fonte_escala: 1.17, ..Default::default() },

        "code" | "kbd" | "samp" | "tt" => EstiloPadrao { monoespaco: true, ..Default::default() },

        "a" => EstiloPadrao {
            cor: Some(Cor(0, 0, 238)), // #0000EE
            sublinhado: true,
            ..Default::default()
        },

        "input" | "button" | "select" | "textarea" | "img" | "video"
        | "audio" | "canvas" | "iframe" | "embed" | "object" | "svg"
        | "meter" | "progress" => EstiloPadrao { display: InlineBlock, ..Default::default() },
        "label" | "legend" => Default::default(),

        // ---- Inline genérico (span e tudo que não conhecemos) ----
        "span" | "abbr" | "q" | "time" | "bdi" | "bdo" | "data" | "wbr"
        | "br" | "ruby" | "rt" | "rp" => Default::default(),

        _ => Default::default(),
    }
}

pub fn eh_oculta(tag: &str) -> bool {
    estilo_padrao(tag).display == Display::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titulos_sao_maiores_e_em_negrito() {
        let h1 = estilo_padrao("h1");
        assert_eq!(h1.display, Display::Block);
        assert!(h1.negrito);
        assert_eq!(h1.fonte_escala, 2.0);
    }

    #[test]
    fn script_e_oculto() {
        assert!(eh_oculta("script"));
        assert!(!eh_oculta("p"));
    }

    #[test]
    fn tag_desconhecida_vira_inline() {
        assert_eq!(estilo_padrao("minha-tag").display, Display::Inline);
    }

    #[test]
    fn link_e_azul_e_sublinhado() {
        let a = estilo_padrao("a");
        assert_eq!(a.cor, Some(Cor(0, 0, 238)));
        assert!(a.sublinhado);
    }
}