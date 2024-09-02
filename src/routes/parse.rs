use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;
use std::collections::HashMap;

#[derive(Parser)]
#[grammar = "routes/grammar.peg"]
struct RouteParser;

fn extract_cfg(pair: Pair<Rule>) -> HashMap<String, String> {
    let mut cfg = HashMap::new();
    for entry in pair.into_inner().flat_map(|p| p.into_inner()) {
        if let (Some(key), Some(value)) = (entry.clone().into_inner().next(), entry.into_inner().nth(1)) {
            cfg.insert(key.as_str().to_string(), value.as_str().trim_matches('"').to_string());
        }
    }
    cfg
}

fn extract_block_content(block: &str) -> String {
    let lines: Vec<&str> = block.lines().collect();
    if lines.len() < 3 {
        return block.trim().trim_matches('{').trim_matches('}').trim().to_string();
    }

    let indent = lines[1].chars().take_while(|c| c.is_whitespace()).count();
    lines[1..lines.len() - 1]
        .iter()
        .map(|line| if line.len() > indent { &line[indent..] } else { line.trim() })
        .collect::<Vec<&str>>()
        .join("\n")
}

fn extract_route_info(pair: Pair<Rule>, input: &str) -> super::Route {
    let mut route_info = super::Route::default();

    for inner_pair in pair.into_inner() {
        match inner_pair.as_rule() {
            Rule::route_attr => {
                for attr_pair in inner_pair.into_inner() {
                    match attr_pair.as_rule() {
                        Rule::string_literal => {
                            route_info.route = attr_pair.as_str().trim_matches('"').into();
                        }
                        Rule::cfg_block => {
                            route_info.cfg = Some(extract_cfg(attr_pair));
                        }
                        _ => {}
                    }
                }
            }
            Rule::function_def => {
                for func_pair in inner_pair.into_inner() {
                    match func_pair.as_rule() {
                        Rule::route_name => {
                            route_info.fn_name = func_pair.as_str().into();

                            if route_info.route.is_empty() {
                                route_info.route = format!("/{}", func_pair.as_str()).into();
                            }
                        }
                        Rule::parameters => {
                            route_info.args = Some(func_pair.into_inner().map(|p| p.as_str().into()).collect());
                        }
                        Rule::block => {
                            route_info.fn_body = extract_block_content(func_pair.as_str()).into();

                            let start_pos = func_pair.as_span().start();
                            let end_pos = func_pair.as_span().end();
                            let file_lines: Vec<&str> = input.lines().collect();

                            route_info.start_pos = file_lines.iter().take_while(|line| input.find(line.to_owned()).unwrap() < start_pos).count() - 1;
                            route_info.end_pos = file_lines.iter().take_while(|line| input.find(line.to_owned()).unwrap() <= end_pos).count() - 1;
                        }
                        _ => {}
                    }
                }
            }
            Rule::block => {
                route_info.fn_body = extract_block_content(inner_pair.as_str()).into();

                let start_pos = inner_pair.as_span().start();
                let end_pos = inner_pair.as_span().end();
                let file_lines: Vec<&str> = input.lines().collect();

                route_info.start_pos = file_lines.iter().take_while(|line| input.find(line.to_owned()).unwrap() < start_pos).count() - 1;
                route_info.end_pos = file_lines.iter().take_while(|line| input.find(line.to_owned()).unwrap() <= end_pos).count() - 1;
            }
            _ => {}
        }
    }

    route_info
}

fn process_pair(pair: Pair<Rule>, input: &str) {
    match pair.as_rule() {
        Rule::index => extract_route_info(pair, input).save(super::RtKind::Index),
        Rule::route_definition => extract_route_info(pair, input).save(super::RtKind::Normal),
        Rule::not_found => extract_route_info(pair, input).save(super::RtKind::NotFound),
        Rule::wildcard => extract_route_info(pair, input).save(super::RtKind::Wildcard),

        _ => {
            for inner_pair in pair.into_inner() {
                process_pair(inner_pair, input);
            }
        }
    }
}

pub fn try_parse(input: &str) {
    match RouteParser::parse(Rule::grammar, input) {
        Ok(pairs) => {
            for pair in pairs {
                process_pair(pair, input);
            }
        }
        Err(e) => println!("Error: {}", e),
    }
}
