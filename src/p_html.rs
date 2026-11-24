use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};



async fn resquest(url: &str) -> Result<String> {
    let body = reqwest::get(url)
        .await?
        .text()
        .await?;

    Ok(body)
}

fn parser(html: &str) {
    let dom = parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .unwrap();

    prc(&dom.document, 0);
}

fn prc(no: &Handle, level: usize) {
    let indent = "  ".repeat(level);

    match &no.data {
        NodeData::Text { contents } => {
            let t = contents.borrow();
            if !t.trim().is_empty() {
                //println!("{indent}texto: {}", t.trim());
            }
        }
        NodeData::Element { name, attrs, .. } => {
            for a in attrs.borrow().iter() {
                if &*name.local == "link" && &*a.name.local == "href" {
                    let full_link = format!("{URL}{}", &*a.value);
                    println!("attr: {} | link: {}", a.name.local, full_link)
                }
                //println!("{indent} attr {} = {}", a.name.local, a.value);
            }
        }
        _ => {}
    }

    for child in no.children.borrow().iter() {
        prc(child, level + 1)
    }
}