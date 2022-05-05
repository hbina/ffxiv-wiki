use rayon::prelude::*;
use std::io::Read;

use markdown_gen::markdown::AsMarkdown;

#[derive(Copy, Clone, Debug)]
pub struct QSSpec;

impl quoted_string::spec::GeneralQSSpec for QSSpec {
    type Quoting = Self;
    type Parsing = QSParse;
}

impl quoted_string::spec::QuotingClassifier for QSSpec {
    fn classify_for_quoting(
        pcp: quoted_string::spec::PartialCodePoint,
    ) -> quoted_string::spec::QuotingClass {
        match pcp.as_u8() {
            b'"' | b'\\' => quoted_string::spec::QuotingClass::NeedsQuoting,
            _ => quoted_string::spec::QuotingClass::QText,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub struct QSParse;

impl quoted_string::spec::ParsingImpl for QSParse {
    fn can_be_quoted(_: quoted_string::spec::PartialCodePoint) -> bool {
        true
    }

    fn handle_normal_state(
        _: quoted_string::spec::PartialCodePoint,
    ) -> Result<(quoted_string::spec::State<Self>, bool), quoted_string::error::CoreError> {
        Ok((quoted_string::spec::State::Normal, true))
    }

    fn advance(
        &self,
        _: quoted_string::spec::PartialCodePoint,
    ) -> Result<(quoted_string::spec::State<Self>, bool), quoted_string::error::CoreError> {
        Ok((quoted_string::spec::State::Normal, false))
    }
}

#[derive(Debug)]
pub enum ContentType {
    Header(String, usize),
    Paragraph(String, bool),
    List(Vec<String>),
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();

    let input_folder = args.get(1).unwrap();
    let output_folder = args.get(2).unwrap();

    let input_files = collect_files(std::path::Path::new(input_folder));

    input_files
        .par_iter()
        .for_each(|input_path| generate_markdown(input_path, std::path::Path::new(output_folder)));
}

fn collect_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut result = Vec::default();
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                result.append(&mut collect_files(&path));
            } else if path.extension().map(|c| c == "html").unwrap_or(false) {
                result.push(path.to_owned());
            }
        }
    } else {
        result.push(dir.to_owned());
    }
    result
}

fn generate_markdown(input_file: &std::path::Path, output_folder: &std::path::Path) -> () {
    let file_name_str = input_file
        .file_name()
        .unwrap()
        .to_owned()
        .into_string()
        .unwrap();

    if file_name_str.starts_with('_') {
        // Skip these files
        // TODO: Figure out what to do with it
        return ();
    }

    let file_name = std::path::Path::new(&file_name_str).to_owned();

    let content = {
        let mut file = std::fs::File::open(&input_file).unwrap();
        let mut buf = String::default();
        file.read_to_string(&mut buf).unwrap();
        buf
    };

    let output_file = {
        let mut output_file = output_folder.to_owned();
        output_file.push(file_name.as_path());
        output_file.set_extension("md");
        output_file
    };

    let file = std::fs::File::create(&output_file).unwrap();
    let mut md = markdown_gen::markdown::Markdown::new(file);

    let document = scraper::Html::parse_document(&content);

    // TITLE

    let title = if let Some(x) = document
        .select(&scraper::Selector::parse("#firstHeading,#section_0").unwrap())
        .next()
    {
        x.inner_html()
    } else {
        "UNKNOWN".to_string()
    };

    md.write_raw(markdown_gen::markdown::RichText::new(
        format!(
            "+++\ntitle= {}\n+++",
            quoted_string::quote::<QSSpec>(&title)
                .map_err(|_| title)
                .unwrap()
        )
        .as_str(),
    ))
    .unwrap();

    // CONTENT

    // Level 1 header
    if let Some(element_ref) = document
        .select(&scraper::Selector::parse(".mw-parser-output").unwrap())
        .next()
    {
        let contents = element_ref
            .children()
            .filter_map(|v| scraper::element_ref::ElementRef::wrap(v))
            .filter_map(|v| match v.value().name() {
                "h2" => Some(ContentType::Header(
                    v.select(&scraper::Selector::parse("h2 > span").unwrap())
                        .next()
                        .unwrap()
                        .inner_html(),
                    1,
                )),
                "h3" => Some(ContentType::Header(
                    v.select(&scraper::Selector::parse("h3 > span").unwrap())
                        .next()
                        .unwrap()
                        .inner_html(),
                    2,
                )),
                "p" => Some(ContentType::Paragraph(
                    v.inner_html(),
                    v.children().all(|v| v.value().is_text()),
                )),
                "section" => Some(ContentType::List(
                    v.select(&scraper::Selector::parse("ul > li").unwrap())
                        .map(|v| v.inner_html())
                        .collect::<Vec<_>>(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();

        for content in contents {
            match content {
                ContentType::Header(text, level) => md.write(text.as_str().heading(level)).unwrap(),
                ContentType::Paragraph(text, is_text_only) => {
                    if is_text_only {
                        md.write(text.as_str().paragraph()).unwrap()
                    } else {
                        // Otherwise need to parse again
                    }
                }
                ContentType::List(list) => {
                    let mut md_list = markdown_gen::markdown::ListOwned::new(false);
                    for x in list {
                        md_list.push(x);
                    }
                    md.write_raw(md_list).unwrap();
                }
            }
        }
    } else {
        // Different kind of page
    }

    // FOOTER

    let relative_path = urlencoding::encode(
        pathdiff::diff_paths(input_file, output_file)
            .unwrap()
            .to_str()
            .unwrap(),
    )
    .to_string();

    md.write(
        "generated from "
            .paragraph()
            .append(file_name.to_str().unwrap().bold().link_to(&relative_path)),
    )
    .unwrap();
}
