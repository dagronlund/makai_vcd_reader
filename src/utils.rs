use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crossbeam::channel::bounded;
use makai::utils::crossbeam::{ReceiverQueued, SenderQueued};
use makai_waveform_db::{errors::WaveformError, Waveform};

use crate::errors::*;
use crate::lexer::{position::LexerPosition, Lexer, LexerToken};
use crate::parser::{VcdEntry, VcdHeader, VcdReader};
use crate::tokenizer::Tokenizer;

#[derive(Debug)]
pub enum VcdError {
    Io(std::io::Error),
    Lexer(LexerPosition),
    Tokenizer(TokenizerError),
    Parser(ParserError),
    Waveform(WaveformError),
}

impl From<std::io::Error> for VcdError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<TokenizerError> for VcdError {
    fn from(err: TokenizerError) -> Self {
        Self::Tokenizer(err)
    }
}

impl From<LexerPosition> for VcdError {
    fn from(pos: LexerPosition) -> Self {
        Self::Lexer(pos)
    }
}

impl From<ParserError> for VcdError {
    fn from(err: ParserError) -> Self {
        Self::Parser(err)
    }
}

impl From<WaveformError> for VcdError {
    fn from(err: WaveformError) -> Self {
        Self::Waveform(err)
    }
}

pub type VcdResult<T> = Result<T, VcdError>;

pub fn load_single_threaded(
    bytes: String,
    status: &mut dyn FnMut((usize, usize)),
) -> VcdResult<(VcdHeader, Waveform)> {
    log::debug!("Loading VCD (single-threaded)...");
    let file_size = bytes.as_bytes().len();
    let mut lexer = Lexer::new(&bytes);
    let mut tokenizer = Tokenizer::new(&bytes);
    let mut parser = VcdReader::new();
    let mut waveform = Waveform::new();
    parser.parse_header(&mut |bs| tokenizer.next(lexer.next_token()?, bs))?;
    parser.get_header().initialize_waveform(&mut waveform);
    log::debug!("Header parsed...");
    let mut last_index = lexer.get_position().get_index();
    status((last_index, file_size));
    loop {
        let entry =
            match parser.parse_waveform(&mut |bs| tokenizer.next(lexer.next_token()?, bs))? {
                Some(entry) => entry,
                None => break,
            };
        match entry {
            VcdEntry::Timestamp(timestamp) => waveform.insert_timestamp(timestamp)?,
            VcdEntry::Vector(bv, idcode) => waveform.update_vector(idcode, bv.clone())?,
            VcdEntry::Real(value, idcode) => waveform.update_real(idcode, value)?,
        }
        let index = lexer.get_position().get_index();
        if (index - last_index) * 200 / file_size > 0 {
            last_index = index;
            status((last_index, file_size));
        }
    }
    log::debug!("VCD loaded!");
    Ok((parser.into_header(), waveform))
}

pub fn load_multi_threaded(
    bytes: String,
    waveform_threads: usize,
    status: Arc<Mutex<(usize, usize)>>,
) -> JoinHandle<VcdResult<(VcdHeader, Waveform)>> {
    let channel_limit = 1024;
    let queue_limit = 4096;
    let file_size = bytes.as_bytes().len();

    let status_clean = status.clone();

    let loader_fn = move || {
        log::debug!("Loading VCD (multi-threaded)...");
        // Create a tokenizer and parser for the file
        let mut lexer = Lexer::new(&bytes);
        let mut tokenizer = Tokenizer::new(&bytes);
        let mut parser = VcdReader::new();
        let mut waveform = Waveform::new();
        *status.lock().unwrap() = (lexer.get_position().get_index(), file_size);
        parser.parse_header(&mut |bs| tokenizer.next(lexer.next_token()?, bs))?;
        parser.get_header().initialize_waveform(&mut waveform);
        *status.lock().unwrap() = (lexer.get_position().get_index(), file_size);
        log::debug!("Header parsed...");

        // Spawn threads for lexing, parsing/tokenizing, and assembling the waveform
        let (tx_lexer, rx_lexer) = bounded::<Vec<LexerToken>>(channel_limit);
        let (mut tx_lexer, mut rx_lexer) = (
            SenderQueued::new(tx_lexer, queue_limit),
            ReceiverQueued::new(rx_lexer),
        );
        let (tx_parser, rx_parser) = bounded::<Vec<VcdEntry>>(channel_limit);
        let (mut tx_parser, mut rx_parser) = (
            SenderQueued::new(tx_parser, queue_limit),
            ReceiverQueued::new(rx_parser),
        );
        let mut waveform_handles: Vec<JoinHandle<Result<Waveform, WaveformError>>> = Vec::new();
        let mut tx_dispatchers = Vec::new();
        for mut waveform_shard in waveform.shard(waveform_threads) {
            let (tx_dispatcher, rx_dispatcher) = bounded(channel_limit);
            let (tx_dispatcher, mut rx_dispatcher) = (
                SenderQueued::new(tx_dispatcher, queue_limit),
                ReceiverQueued::new(rx_dispatcher),
            );
            tx_dispatchers.push(tx_dispatcher);
            waveform_handles.push(thread::spawn(move || loop {
                match rx_dispatcher.recv().unwrap() {
                    Some(VcdEntry::Timestamp(timestamp)) => {
                        waveform_shard.insert_timestamp(timestamp)?
                    }
                    Some(VcdEntry::Vector(value, id)) => waveform_shard.update_vector(id, value)?,
                    Some(VcdEntry::Real(value, id)) => waveform_shard.update_real(id, value)?,
                    None => return Ok(waveform_shard),
                }
            }));
        }
        let parser_handle = thread::spawn(move || loop {
            match parser.parse_waveform(&mut |bs| tokenizer.next(rx_lexer.recv().unwrap(), bs)) {
                Ok(Some(entry)) => tx_parser.send(entry).unwrap(),
                Ok(None) => {
                    tx_parser.finish().unwrap();
                    return Ok(parser);
                }
                Err(err) => {
                    tx_parser.finish().unwrap();
                    return Err(err);
                }
            }
        });
        let dispatcher_handle = thread::spawn(move || loop {
            match rx_parser.recv().unwrap() {
                Some(entry) => match entry {
                    VcdEntry::Timestamp(timestamp) => {
                        for tx_dispatcher in &mut tx_dispatchers {
                            tx_dispatcher.send(VcdEntry::Timestamp(timestamp)).unwrap();
                        }
                    }
                    VcdEntry::Vector(value, id) => {
                        tx_dispatchers[id % waveform_threads]
                            .send(VcdEntry::Vector(value, id))
                            .unwrap();
                    }
                    VcdEntry::Real(value, id) => {
                        tx_dispatchers[id % waveform_threads]
                            .send(VcdEntry::Real(value, id))
                            .unwrap();
                    }
                },
                None => {
                    for tx_dispatcher in tx_dispatchers {
                        tx_dispatcher.finish().unwrap();
                    }
                    return;
                }
            }
        });

        let mut last_index = lexer.get_position().get_index();
        loop {
            match lexer.next_token() {
                Ok(Some(lexer_token)) => {
                    tx_lexer.send(lexer_token).unwrap();
                    let index = lexer.get_position().get_index();
                    if (index - last_index) * 200 / file_size > 0 {
                        *status.lock().unwrap() = (index, file_size);
                        last_index = index;
                    }
                }
                Ok(None) => {
                    tx_lexer.finish().unwrap();
                    *status.lock().unwrap() = (file_size, file_size);
                    break;
                }
                Err(err) => {
                    tx_lexer.finish().unwrap();
                    return Err(VcdError::from(err));
                }
            }
        }
        let parser = parser_handle.join().unwrap()?;
        dispatcher_handle.join().unwrap();
        let mut waveform_shards = Vec::new();
        for handle in waveform_handles {
            waveform_shards.push(handle.join().unwrap()?);
        }
        log::debug!("Body parsed...");
        let waveform = Waveform::unshard(waveform_shards)?;
        log::debug!("Shards combined...");
        Ok((parser.into_header(), waveform))
    };

    thread::spawn(move || match loader_fn() {
        Ok(ok) => {
            log::debug!("VCD loaded!");
            Ok(ok)
        }
        Err(err) => {
            log::error!("VCD error: {err:?}");
            *status_clean.lock().unwrap() = (file_size, file_size);
            Err(err)
        }
    })
}
