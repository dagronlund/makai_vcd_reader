use std::collections::HashMap;
use std::fs;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use colored::*;
use humansize::{format_size, DECIMAL};
use indicatif::ProgressBar;
use log::*;
use simple_logger::SimpleLogger;

use makai::utils::bytes::ByteStorage;
use makai_vcd_reader::errors::*;
use makai_vcd_reader::lexer::position::*;
use makai_vcd_reader::lexer::*;
use makai_vcd_reader::parser::*;
use makai_vcd_reader::tokenizer::token::*;
use makai_vcd_reader::tokenizer::*;
use makai_vcd_reader::utils::*;
use makai_waveform_db::errors::*;
use makai_waveform_db::*;

pub struct ProgressBarLimiter {
    pb: ProgressBar,
    step: u64,
}

impl ProgressBarLimiter {
    pub fn new(size: u64, divider: u64) -> Self {
        Self {
            pb: ProgressBar::new(size),
            step: size / divider,
        }
    }

    pub fn get(&self) -> &ProgressBar {
        &self.pb
    }

    pub fn set_position(&self, pos: u64) {
        if pos - self.pb.position() > self.step {
            self.pb.set_position(pos);
        }
    }

    pub fn finish(&self) {
        self.pb.finish();
    }
}

#[derive(Debug)]
enum TestError {
    Io(io::Error),
    Vcd(VcdError),
}

impl From<io::Error> for TestError {
    fn from(err: io::Error) -> Self {
        TestError::Io(err)
    }
}

impl From<VcdError> for TestError {
    fn from(err: VcdError) -> Self {
        TestError::Vcd(err)
    }
}

impl From<TokenizerError> for TestError {
    fn from(err: TokenizerError) -> Self {
        Self::Vcd(VcdError::Tokenizer(err))
    }
}

impl From<LexerPosition> for TestError {
    fn from(pos: LexerPosition) -> Self {
        Self::Vcd(VcdError::Lexer(pos))
    }
}

impl From<ParserError> for TestError {
    fn from(err: ParserError) -> Self {
        Self::Vcd(VcdError::Parser(err))
    }
}

impl From<WaveformError> for TestError {
    fn from(err: WaveformError) -> Self {
        Self::Vcd(VcdError::Waveform(err))
    }
}

type TestResult<T> = Result<T, TestError>;

fn print_token_highlighted(t: &Token, bs: &ByteStorage) -> TestResult<()> {
    let mut s = Vec::new();
    t.write_to(&bs, &mut s)?;

    match t {
        Token::Comment(_, _) | Token::Date(_, _) | Token::Version(_, _) => {
            print!("{}", String::from_utf8_lossy(&s).yellow());
        }
        Token::Scope {
            scope_type: _,
            scope_id: _,
            pos: _,
        }
        | Token::Timescale {
            timescale: _,
            offset: _,
            pos: _,
        }
        | Token::Var {
            net_type: _,
            width: _,
            token_idcode: _,
            variable_description: _,
            pos: _,
        }
        | Token::UpScope(_)
        | Token::EndDefinitions(_) => {
            print!("{}", String::from_utf8_lossy(&s).cyan());
        }
        Token::DumpAll(_)
        | Token::DumpOff(_)
        | Token::DumpOn(_)
        | Token::DumpVars(_)
        | Token::End(_) => {
            print!("{}", String::from_utf8_lossy(&s).magenta());
        }
        Token::Timestamp(_, _) => {
            print!("{}", String::from_utf8_lossy(&s).green());
        }
        Token::VectorValue(bv, _, _) => {
            if bv.get_bit_width() == 1 {
                print!("{}", String::from_utf8_lossy(&s).red());
            } else {
                print!("{}", String::from_utf8_lossy(&s).red().bold());
            }
        }
        Token::RealValue(_, _, _) => {
            print!("{}", String::from_utf8_lossy(&s).blue());
        }
    }

    Ok(())
}

#[test]
fn test_tokenizer() -> TestResult<()> {
    let _ = SimpleLogger::new().env().init();

    let fname = "res/gecko.vcd";

    let file_usize = fs::metadata(fname).unwrap().len();
    let bar = ProgressBarLimiter::new(file_usize, 200);

    let bytes = fs::read_to_string(fname)?;
    // let mut file = BufWriter::with_capacity(1 << 20, tempfile::tempfile()?);

    let start = Instant::now();
    let mut lexer = Lexer::new(&bytes);
    let mut tokenizer = Tokenizer::new(&bytes);
    let mut bs = ByteStorage::new();

    let mut tokens = Vec::new();
    loop {
        match tokenizer.next(lexer.next_token()?, &mut bs) {
            Ok(Some(t)) => {
                tokens.push(t);
                bar.set_position(lexer.get_position().get_index() as u64);
            }
            Ok(None) => {
                bar.finish();
                info!("Done: {:?}!", start.elapsed());
                break;
            }
            Err(err) => {
                bar.finish();
                error!("\n{:?}\n", err);
                return Err(TestError::Vcd(VcdError::Tokenizer(err)));
            }
        }
    }

    println!("Result:");
    for t in &tokens {
        print_token_highlighted(t, &bs)?;
    }

    Ok(())
}

#[test]
fn test_parser() -> TestResult<()> {
    let _ = SimpleLogger::new().env().init();
    info!("test_parser...");
    let fname = "res/gecko.vcd";

    let file_usize = fs::metadata(fname).unwrap().len();
    let bar = ProgressBarLimiter::new(file_usize, 200);

    let bytes = fs::read_to_string(fname)?;

    let start = Instant::now();
    let mut lexer = Lexer::new(&bytes);
    let mut tokenizer = Tokenizer::new(&bytes);
    let mut parser = VcdReader::new();
    parser.parse_header(&mut |bs| tokenizer.next(lexer.next_token()?, bs))?;
    info!("Parsing header done! ({:?})", start.elapsed());
    let start = Instant::now();

    info!(
        "Version: {}",
        parser.get_header().get_version().clone().unwrap()
    );
    info!("Date: {}", parser.get_header().get_date().clone().unwrap());
    info!(
        "Timescale: {}",
        parser.get_header().get_timescale().unwrap()
    );

    let mut tokens = Vec::new();
    loop {
        match tokenizer.next(lexer.next_token()?, parser.get_byte_storage_mut()) {
            Ok(Some(t)) => {
                tokens.push(t);
                bar.set_position(lexer.get_position().get_index() as u64);
            }
            Ok(None) => {
                bar.finish();
                info!("Done: {:?}!", start.elapsed());
                break;
            }
            Err(err) => {
                bar.finish();
                error!("\n{:?}\n", err);
                return Err(TestError::Vcd(VcdError::Tokenizer(err)));
            }
        }
    }

    Ok(())
}

#[test]
fn test_waveform() -> TestResult<()> {
    let _ = SimpleLogger::new().env().init();
    info!("test_waveform...");
    let fname = "res/gecko.vcd";

    let file_usize = fs::metadata(fname).unwrap().len();

    let bytes = fs::read_to_string(fname)?;

    let start = Instant::now();
    let mut lexer = Lexer::new(&bytes);
    let mut tokenizer = Tokenizer::new(&bytes);
    let mut parser = VcdReader::new();
    info!("Parsing header...");
    parser.parse_header(&mut |bs| tokenizer.next(lexer.next_token()?, bs))?;
    info!("Parsing header done! ({:?})", start.elapsed());
    let start = Instant::now();

    info!(
        "Version: {}",
        parser.get_header().get_version().clone().unwrap()
    );
    info!("Date: {}", parser.get_header().get_date().clone().unwrap());
    info!(
        "Timescale: {}",
        parser.get_header().get_timescale().unwrap()
    );

    let mut waveform = Waveform::new();
    let mut vector_map = HashMap::new();
    let mut real_map = HashMap::new();

    for (idcode, width) in parser.get_header().get_idcodes_map().iter() {
        match width {
            VcdVariableWidth::Vector { width } => {
                waveform.initialize_vector(*idcode, *width);
                vector_map.insert(*idcode, Vec::new());
            }
            VcdVariableWidth::Real => {
                waveform.initialize_real(*idcode);
                real_map.insert(*idcode, Vec::new());
            }
        }
    }

    let mut current_timestamp = None;

    info!("Parsing changes...");
    let bar = ProgressBarLimiter::new(file_usize, 200);
    loop {
        let entry =
            match parser.parse_waveform(&mut |bs| tokenizer.next(lexer.next_token()?, bs))? {
                Some(entry) => entry,
                None => {
                    bar.finish();
                    info!("Done: {:?}!", start.elapsed());
                    break;
                }
            };
        match entry {
            VcdEntry::Timestamp(timestamp) => {
                waveform.insert_timestamp(timestamp)?;
                current_timestamp = Some(timestamp);
            }
            VcdEntry::Vector(bv, idcode) => {
                waveform.update_vector(idcode, bv.clone())?;
                vector_map
                    .get_mut(&idcode)
                    .unwrap()
                    .push((current_timestamp.unwrap(), bv));
            }
            VcdEntry::Real(value, idcode) => {
                waveform.update_real(idcode, value)?;
                real_map
                    .get_mut(&idcode)
                    .unwrap()
                    .push((current_timestamp, value));
            }
        }
        bar.set_position(lexer.get_position().get_index() as u64);
    }

    info!("Verifying waveform against changes...");
    let bar = ProgressBarLimiter::new(vector_map.keys().len() as u64, 200);
    let start = Instant::now();
    let mut i = 0;
    for (idcode, changes) in &vector_map {
        let signal = waveform.get_vector_signal(*idcode).unwrap();
        let mut signal_iter = signal.get_history().into_iter();
        let mut changes_iter = changes.into_iter();
        let mut value_index = 0;
        loop {
            let (signal_timestamp, change_timestamp, signal_index, change_bitvector) =
                match (signal_iter.next(), changes_iter.next()) {
                    (Some(signal_index), Some((change_timestamp, change_bitvector))) => (
                        signal_index.get_timestamp_index(),
                        change_timestamp,
                        signal_index.get_value_index(),
                        change_bitvector,
                    ),
                    (Some(signal_index), None) => {
                        panic!(
                            "More signal timestamps! {}",
                            signal_index.get_timestamp_index()
                        );
                    }
                    (None, Some((change_timestamp, _))) => {
                        panic!("More change timestamps! {}", change_timestamp);
                    }
                    (None, None) => break,
                };
            let signal_timestamp = waveform.get_timestamps()[signal_timestamp];
            if signal_timestamp != *change_timestamp {
                panic!(
                    "Signal Timestamp ({}) != Change Timestamp ({})",
                    signal_timestamp, change_timestamp
                );
            }
            if signal_index != value_index {
                panic!(
                    "Signal index ({}) not incrementing, should be ({})",
                    signal_index, value_index
                );
            }
            let signal_bitvector = signal.get_bitvector(signal_index);
            if &signal_bitvector != change_bitvector {
                panic!(
                    "Idcode: {}, Signal: {:?} != Change: {:?}",
                    *idcode, &signal_bitvector, change_bitvector
                );
            }
            value_index += 1;
        }
        i += 1;
        bar.set_position(i);
    }
    bar.finish();
    info!("Done: {:?}!", start.elapsed());

    Ok(())
}

#[test]
fn test_perf() -> TestResult<()> {
    use std::thread;

    let _ = SimpleLogger::new().env().init();
    info!("test_perf...");
    let fname = "res/gecko.vcd";

    let bytes = fs::read_to_string(fname)?;
    let file_size = bytes.as_bytes().len();

    info!("Single-threaded performance:");
    let start = Instant::now();
    let bar = ProgressBarLimiter::new(file_size as u64, 200);
    bar.set_position(0);
    let (_, waveform) = load_single_threaded(bytes, &mut |(partial, _)| {
        bar.set_position(partial as u64);
    })?;
    bar.finish();
    let elapsed = start.elapsed();
    info!(
        "Timestamps: {} / {:?}",
        waveform.get_timestamps().len(),
        elapsed
    );
    info!(
        "Performance: {}/s",
        format_size((file_size as f64 / elapsed.as_secs_f64()) as usize, DECIMAL)
    );

    // Read VCD file header and build out waveform structure
    let bytes = fs::read_to_string(fname)?;
    let file_size = bytes.as_bytes().len();

    info!("Multi-threaded performance:");
    let start = Instant::now();
    let bar = ProgressBarLimiter::new(file_size as u64, 200);
    bar.set_position(0);
    let status = Arc::new(Mutex::new((0, 0)));
    let handle = load_multi_threaded(bytes, 4, status.clone());
    loop {
        let (pos, total) = *status.lock().unwrap();
        bar.set_position(pos as u64);
        if pos >= total && total > 0 {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    let (_, waveform) = handle.join().unwrap()?;
    bar.finish();
    let elapsed = start.elapsed();
    info!(
        "Timestamps: {} / {:?}",
        waveform.get_timestamps().len(),
        elapsed
    );
    info!(
        "Performance: {}/s",
        format_size((file_size as f64 / elapsed.as_secs_f64()) as usize, DECIMAL)
    );

    info!("Waveform timestamps: {}", waveform.get_timestamps().len());

    info!(
        "Block Size {}, Vector Size {}",
        format_size(waveform.get_block_size(), DECIMAL),
        format_size(waveform.get_vector_size(), DECIMAL),
    );

    info!(
        "Empty Vectors: {}, One Vectors: {} * 512B = {}",
        waveform.count_empty(),
        waveform.count_one(),
        format_size(waveform.count_one() * 512, DECIMAL)
    );

    Ok(())
}

#[test]
fn test_waveform_search() -> TestResult<()> {
    use std::thread;

    let _ = SimpleLogger::new().env().init();
    info!("test_waveform_search...");
    let fname = "res/gecko.vcd";

    // Read VCD file header and build out waveform structure
    let bytes = fs::read_to_string(fname)?;
    let file_size = bytes.as_bytes().len();
    let bar = ProgressBarLimiter::new(file_size as u64, 200);
    bar.set_position(0);
    let status = Arc::new(Mutex::new((0, 0)));
    let handle = load_multi_threaded(bytes, 4, status.clone());
    loop {
        let (pos, total) = *status.lock().unwrap();
        bar.set_position(pos as u64);
        if pos >= total && total > 0 {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    let (header, waveform) = handle.join().unwrap()?;
    bar.finish();

    let _scope = header.get_scope("TOP.gecko_nano_wrapper").unwrap();
    let variable = header.get_variable("TOP.exit_code").unwrap();

    let _signal = match waveform.get_signal(variable.get_idcode()) {
        Some(WaveformSignalResult::Vector(signal)) => signal,
        _ => panic!("Cannot find vector signal!"),
    };

    let signal = match waveform.get_signal(header.get_variable("TOP.clk").unwrap().get_idcode()) {
        Some(WaveformSignalResult::Vector(signal)) => signal,
        _ => panic!("Cannot find clk signal!"),
    };

    let rst_idcode = header.get_variable("TOP.rst").unwrap().get_idcode();
    println!("rst idcode: {rst_idcode}",);

    for i in 0..waveform.get_timestamps().len() {
        let pos = signal
            .get_history()
            .search_timestamp_index(i, WaveformSearchMode::Before)
            .unwrap();
        let _ = signal.get_bitvector(pos.get_value_index());
    }

    let signal = match waveform.get_signal(header.get_variable("TOP.rst").unwrap().get_idcode()) {
        Some(WaveformSignalResult::Vector(signal)) => signal,
        _ => panic!("Cannot find rst signal!"),
    };

    for pos in signal.get_history().into_iter() {
        println!("Pos: {pos:?}");
    }
    println!("{:?}", signal.get_bitvector(0));
    println!("{:?}", signal.get_bitvector(1));

    println!(
        "Value: {:?}",
        waveform.search_value(rst_idcode, 0, WaveformSearchMode::Before)
    );
    println!(
        "Value: {:?}",
        waveform.search_value(rst_idcode, 40, WaveformSearchMode::Before)
    );
    println!(
        "Value: {:?}",
        waveform.search_value(rst_idcode, 200, WaveformSearchMode::Before)
    );

    println!(
        "39: {} 40: {}",
        waveform.get_timestamps()[39],
        waveform.get_timestamps()[40]
    );

    Ok(())
}
