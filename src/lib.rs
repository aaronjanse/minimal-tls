mod structures;

use std::io::Read;
use std::io::Write;

use structures::{Random, ClientHello, CipherSuite, Extension, ContentType, TLSPlaintext, TLSState, TLSError};

// Misc. functions
pub fn bytes_to_u16(bytes : &[u8]) -> u16 {
	((bytes[0] as u16) << 8) | (bytes[1] as u16)
}

// Each connection needs to have its own TLS_config object
pub struct TLS_config<'a> {
	reader : &'a mut Read,
	writer : &'a mut Write,

	state : TLSState,

	// Cache any remaining bytes in a TLS record
	recordcache: Vec<u8>
}

fn tls_init<'a, R : Read, W : Write>(read : &'a mut R, write : &'a mut W) -> TLS_config<'a> {
	TLS_config{reader : read, writer : write, state : TLSState::Start, recordcache : Vec::new() }
}

#[allow(unused_variables)]
#[allow(dead_code)]
impl<'a> TLS_config<'a> {

	fn get_next_tlsplaintext(&mut self) -> Result<TLSPlaintext, TLSError> {
		// Try to read TLSPlaintext header
		let mut buffer : [u8; 5] = [0; 5];
		try!(self.reader.read_exact(&mut buffer).or(Err(TLSError::ReadError)));

		// Match content type (is there a better way to do this in Rust stable?)
		let contenttype : ContentType = match buffer[0] {
			0  => ContentType::InvalidReserved,
			20 => ContentType::ChangeCipherSpecReserved,
			21 => ContentType::Alert,
			22 => ContentType::Handshake,
			23 => ContentType::ApplicationData,
			_  => return Err(TLSError::InvalidHandshakeError)
		};

		// Match legacy protocol version
		let legacy_version = bytes_to_u16(&buffer[1..3]);
		if legacy_version != 0x0301 {
			return Err(TLSError::InvalidHandshakeError)
		}

		// Make sure length is less than 2^14-1
		let length = bytes_to_u16(&buffer[3..5]);
		if length >= 16384 {
			return Err(TLSError::InvalidHandshakeError)
		}

		// Read the remaining data from the buffer
		let mut data = Vec::with_capacity(length as usize);
		try!(self.reader.read_exact(data.as_mut_slice()).or(Err(TLSError::ReadError)));

		Ok(TLSPlaintext{ctype: contenttype, legacy_record_version: legacy_version, length: length, fragment: data})
	}

	fn process_ciphersuites(&mut self, data : &[u8]) -> Result<Vec<CipherSuite>, TLSError> {
		Err(TLSError::InvalidState)
	}
	
	fn process_extensions(&mut self, data : &[u8]) -> Result<Vec<Extension>, TLSError> {
		Err(TLSError::InvalidState)
	}

	// FIXME: When this function checks for length, it should validate that the vector
	// length is a multiple of the vector element size. Otherwise we could lose alignment
	// in the fragment data

    /*
        FIXME: We don't handle fragmented ClientHello messages! Ideally, rather than reading
        directly from the fragment field of a TLSPlaintext, we should use some helper method
        with an offset and # of bytes to read. If offset > remaining_length, the helper
        automatically tries to read another TLSPlaintext
    */
	fn read_clienthello(&mut self) -> Result<ClientHello, TLSError> {
		// First check if we have any cached data from the last record
		if self.recordcache.len() > 0 {
			// TODO: Try to parse from existing cached message
			Err(TLSError::InvalidState)
		} else {
			let mut plaintext : TLSPlaintext = try!(self.get_next_tlsplaintext());

			// Make sure we are dealing with a Handshake TLSPlaintext
			if plaintext.ctype != ContentType::Handshake {
				return Err(TLSError::InvalidMessage)
			}

			// Try to grab a clienthello from the TLSPlaintext
			let legacy_version: u16 = bytes_to_u16(&plaintext.fragment[0..2]);
			if legacy_version != 0x0303 {
				return Err(TLSError::InvalidHandshakeError)
			}

			// The client random must be exactly 32 bytes
			let mut random : Random = [0; 32];
			random.clone_from_slice(&plaintext.fragment.as_mut_slice()[2..34]);

			// Legacy session ID can be 0-32 bytes
			let lsi_length : usize = bytes_to_u16(&plaintext.fragment[34..36]) as usize;
			if lsi_length > 32 {
				return Err(TLSError::InvalidHandshakeError)
			}

			let legacy_session_id : Vec<u8> = plaintext.fragment[36..(36+lsi_length)].to_vec();

			let curr_offset = 36 + lsi_length;

			// Read in the list of valid cipher suites
			// In reality, for TLS 1.3, there are only 5 valid cipher suites, so this list
			// should never have more than 5 elements (10 bytes) in it.
			let cslist_length : usize = bytes_to_u16(&plaintext.fragment[curr_offset..(curr_offset + 2)]) as usize;
			if cslist_length < 2 || cslist_length > (2^16 - 2) {
				return Err(TLSError::InvalidHandshakeError)
			}

			let curr_offset = curr_offset + 2;

			// Process the list of ciphersuites -- in particular, minimal-TLS doesn't support the full list
			let ciphersuites : Vec<CipherSuite> = try!(self.process_ciphersuites(&plaintext.fragment[curr_offset..(curr_offset+cslist_length)]));

			let curr_offset = curr_offset + cslist_length;

			// Read in legacy compression methods
			// For TLS 1.3, this must be a vector with length one (1) containing a null byte
			let comp_length = plaintext.fragment[curr_offset] as usize;

			if comp_length != 1 {
				return Err(TLSError::InvalidHandshakeError)
			}

			if plaintext.fragment[curr_offset+1] != 0x00 {
				return Err(TLSError::InvalidHandshakeError)
			}

			let curr_offset = curr_offset + 2;

			// Parse ClientHello extensions
			let ext_length = bytes_to_u16(&plaintext.fragment[curr_offset..(curr_offset+2)]) as usize;
			if ext_length < 8 || ext_length > 2^16-1 {
				return Err(TLSError::InvalidHandshakeError)
			}

			let curr_offset = curr_offset + 2;

			let extensions : Vec<Extension> = try!(self.process_extensions(&plaintext.fragment[curr_offset..(curr_offset+ext_length)]));

            let curr_offset = curr_offset + ext_length;

			Ok(ClientHello{
                legacy_version: legacy_version,
                random: random,
                legacy_session_id: legacy_session_id,
                cipher_suites: ciphersuites,
                legacy_compression_methods: vec![0],
                extensions: extensions
            })
		}
	}

	fn transition(&mut self) -> Result<&TLS_config, TLSError> {
		match self.state {
			TLSState::Start => {
				// Try to recieve the ClientHello
				let clienthello : ClientHello = try!(self.read_clienthello());
				Err(TLSError::InvalidState)
			},
			TLSState::RecievedClientHello => {
				Err(TLSError::InvalidState)
			},
			_ => {
				Err(TLSError::InvalidState)
			}
		}
	}

	pub fn tls_start(&mut self) -> Result<u8, TLSError>{

		// Ensure we are in the "start" state
		if self.state != TLSState::Start {
			return Err(TLSError::InvalidState)
		}

		/*
			We want to transition through the TLS state machine until we
			encounter an error, or complete the handshake
		*/
		loop {
			match self.transition() {
				Err(e) => return Err(e),
				_ => {
					if self.state == TLSState::Connected {
						break
					}
				}
			}
		};

		// If all goes well, we should be in the "connected" state
		if self.state != TLSState::Connected {
			return Err(TLSError::InvalidState)
		}

		Ok(0)
	}
}

// Ideas for functions...
// TLS_start -> handshake and connection setup
// TLS_send -> sends plaintext
// TLS_recieve -> recieves plaintext
// TLS_end -> closes the connection

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
    }
}
