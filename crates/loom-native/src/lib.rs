//! Safe dynamic-library loading boundary for optional native runtimes.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::{Path, PathBuf};

use libloading::{Library, Symbol};
use loom_types::{Code, LoomError, Result};

type U32Function = unsafe extern "C" fn() -> u32;
type StaticUtf8Function = unsafe extern "C" fn() -> *const c_char;
type JsonHandleFunction = unsafe extern "C" fn(*const c_char, *mut u64) -> u32;
type JsonBufferFunction =
    unsafe extern "C" fn(u64, *const c_char, *mut c_char, usize, *mut usize) -> u32;
type U64Function = unsafe extern "C" fn(u64);

#[derive(Debug)]
pub struct NativeLibrary {
    path: PathBuf,
    library: Library,
}

impl NativeLibrary {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        // SAFETY: Loading a native library can run platform loader initializers. This crate is the
        // approved boundary for that operation; callers must pass an explicit runtime bundle path.
        let library = unsafe { Library::new(path) }.map_err(|error| {
            LoomError::new(
                Code::Io,
                format!("failed to load native library {}: {error}", path.display()),
            )
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            library,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn require_symbol(&self, symbol: &CStr) -> Result<()> {
        // SAFETY: This only validates symbol presence. The raw symbol is not called or exposed.
        unsafe { self.library.get::<*const ()>(symbol.to_bytes_with_nul()) }
            .map(|_| ())
            .map_err(|error| self.symbol_error(symbol, error))
    }

    pub fn require_symbol_name(&self, symbol: &str) -> Result<()> {
        let symbol = symbol_c_string(symbol)?;
        self.require_symbol(&symbol)
    }

    pub fn require_symbols(&self, symbols: &[&CStr]) -> Result<()> {
        for symbol in symbols {
            self.require_symbol(symbol)?;
        }
        Ok(())
    }

    pub fn require_symbol_names(&self, symbols: &[&str]) -> Result<()> {
        for symbol in symbols {
            self.require_symbol_name(symbol)?;
        }
        Ok(())
    }

    pub fn load_u32_function(&self, symbol: &CStr) -> Result<NativeU32Function<'_>> {
        // SAFETY: The returned wrapper is specialized to the exact ABI shape
        // `unsafe extern "C" fn() -> u32` and keeps the library borrow alive.
        let function = unsafe {
            self.library
                .get::<U32Function>(symbol.to_bytes_with_nul())
                .map_err(|error| self.symbol_error(symbol, error))?
        };
        Ok(NativeU32Function {
            name: symbol_name(symbol),
            function,
        })
    }

    pub fn load_u32_function_name(&self, symbol: &str) -> Result<NativeU32Function<'_>> {
        let symbol = symbol_c_string(symbol)?;
        self.load_u32_function(&symbol)
    }

    pub fn load_static_utf8_function(&self, symbol: &CStr) -> Result<NativeStaticUtf8Function<'_>> {
        // SAFETY: The returned wrapper is specialized to the exact ABI shape
        // `unsafe extern "C" fn() -> *const c_char` and keeps the library borrow alive.
        let function = unsafe {
            self.library
                .get::<StaticUtf8Function>(symbol.to_bytes_with_nul())
                .map_err(|error| self.symbol_error(symbol, error))?
        };
        Ok(NativeStaticUtf8Function {
            name: symbol_name(symbol),
            function,
        })
    }

    pub fn load_static_utf8_function_name(
        &self,
        symbol: &str,
    ) -> Result<NativeStaticUtf8Function<'_>> {
        let symbol = symbol_c_string(symbol)?;
        self.load_static_utf8_function(&symbol)
    }

    pub fn load_json_handle_function(&self, symbol: &CStr) -> Result<NativeJsonHandleFunction<'_>> {
        // SAFETY: The returned wrapper is specialized to the exact ABI shape
        // `unsafe extern "C" fn(*const c_char, *mut u64) -> u32`.
        let function = unsafe {
            self.library
                .get::<JsonHandleFunction>(symbol.to_bytes_with_nul())
                .map_err(|error| self.symbol_error(symbol, error))?
        };
        Ok(NativeJsonHandleFunction {
            name: symbol_name(symbol),
            function,
        })
    }

    pub fn load_json_handle_function_name(
        &self,
        symbol: &str,
    ) -> Result<NativeJsonHandleFunction<'_>> {
        let symbol = symbol_c_string(symbol)?;
        self.load_json_handle_function(&symbol)
    }

    pub fn load_json_buffer_function(&self, symbol: &CStr) -> Result<NativeJsonBufferFunction<'_>> {
        // SAFETY: The returned wrapper is specialized to the exact ABI shape
        // `unsafe extern "C" fn(u64, *const c_char, *mut c_char, usize, *mut usize) -> u32`.
        let function = unsafe {
            self.library
                .get::<JsonBufferFunction>(symbol.to_bytes_with_nul())
                .map_err(|error| self.symbol_error(symbol, error))?
        };
        Ok(NativeJsonBufferFunction {
            name: symbol_name(symbol),
            function,
        })
    }

    pub fn load_json_buffer_function_name(
        &self,
        symbol: &str,
    ) -> Result<NativeJsonBufferFunction<'_>> {
        let symbol = symbol_c_string(symbol)?;
        self.load_json_buffer_function(&symbol)
    }

    pub fn load_u64_function(&self, symbol: &CStr) -> Result<NativeU64Function<'_>> {
        // SAFETY: The returned wrapper is specialized to the exact ABI shape
        // `unsafe extern "C" fn(u64)` and keeps the library borrow alive.
        let function = unsafe {
            self.library
                .get::<U64Function>(symbol.to_bytes_with_nul())
                .map_err(|error| self.symbol_error(symbol, error))?
        };
        Ok(NativeU64Function {
            name: symbol_name(symbol),
            function,
        })
    }

    pub fn load_u64_function_name(&self, symbol: &str) -> Result<NativeU64Function<'_>> {
        let symbol = symbol_c_string(symbol)?;
        self.load_u64_function(&symbol)
    }

    fn symbol_error(&self, symbol: &CStr, error: libloading::Error) -> LoomError {
        LoomError::unsupported(format!(
            "native library {} does not expose symbol {}: {error}",
            self.path.display(),
            symbol_name(symbol)
        ))
    }
}

#[derive(Debug)]
pub struct NativeU32Function<'library> {
    name: String,
    function: Symbol<'library, U32Function>,
}

impl NativeU32Function<'_> {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn call(&self) -> u32 {
        // SAFETY: Construction validates the symbol has the ABI shape represented by this wrapper.
        unsafe { (self.function)() }
    }
}

#[derive(Debug)]
pub struct NativeStaticUtf8Function<'library> {
    name: String,
    function: Symbol<'library, StaticUtf8Function>,
}

impl NativeStaticUtf8Function<'_> {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn call_string(&self) -> Result<String> {
        // SAFETY: Construction validates the symbol has the ABI shape represented by this wrapper.
        let pointer = unsafe { (self.function)() };
        if pointer.is_null() {
            return Err(LoomError::corrupt(format!(
                "native symbol {} returned null",
                self.name
            )));
        }
        // SAFETY: The adapter ABI requires a non-null pointer to NUL-terminated static UTF-8.
        let value = unsafe { CStr::from_ptr(pointer) };
        value.to_str().map(str::to_string).map_err(|error| {
            LoomError::corrupt(format!(
                "native symbol {} returned invalid UTF-8: {error}",
                self.name
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeStatusHandle {
    pub status: u32,
    pub handle: u64,
}

#[derive(Debug)]
pub struct NativeJsonHandleFunction<'library> {
    name: String,
    function: Symbol<'library, JsonHandleFunction>,
}

impl NativeJsonHandleFunction<'_> {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn call(&self, request_json: &str) -> Result<NativeStatusHandle> {
        let request = symbol_c_string(request_json)?;
        let mut handle = 0_u64;
        // SAFETY: Construction validates the symbol has the ABI shape represented by this wrapper.
        let status = unsafe { (self.function)(request.as_ptr(), &mut handle) };
        Ok(NativeStatusHandle { status, handle })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeStatusJson {
    pub status: u32,
    pub json: Option<String>,
    pub required_len: usize,
}

#[derive(Debug)]
pub struct NativeJsonBufferFunction<'library> {
    name: String,
    function: Symbol<'library, JsonBufferFunction>,
}

impl NativeJsonBufferFunction<'_> {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn call(
        &self,
        handle: u64,
        request_json: &str,
        output_capacity: usize,
    ) -> Result<NativeStatusJson> {
        let request = symbol_c_string(request_json)?;
        let mut required_len = 0_usize;
        let mut output = vec![0_u8; output_capacity];
        let output_pointer = if output.is_empty() {
            std::ptr::null_mut()
        } else {
            output.as_mut_ptr().cast::<c_char>()
        };
        // SAFETY: Construction validates the symbol has the ABI shape represented by this wrapper.
        let status = unsafe {
            (self.function)(
                handle,
                request.as_ptr(),
                output_pointer,
                output.len(),
                &mut required_len,
            )
        };
        let json = if status == 0 {
            let terminator = output.iter().position(|byte| *byte == 0).ok_or_else(|| {
                LoomError::corrupt(format!(
                    "native symbol {} returned JSON without NUL terminator",
                    self.name
                ))
            })?;
            Some(
                std::str::from_utf8(&output[..terminator])
                    .map(str::to_string)
                    .map_err(|error| {
                        LoomError::corrupt(format!(
                            "native symbol {} returned invalid UTF-8: {error}",
                            self.name
                        ))
                    })?,
            )
        } else {
            None
        };
        Ok(NativeStatusJson {
            status,
            json,
            required_len,
        })
    }
}

#[derive(Debug)]
pub struct NativeU64Function<'library> {
    name: String,
    function: Symbol<'library, U64Function>,
}

impl NativeU64Function<'_> {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn call(&self, value: u64) {
        // SAFETY: Construction validates the symbol has the ABI shape represented by this wrapper.
        unsafe { (self.function)(value) }
    }
}

fn symbol_name(symbol: &CStr) -> String {
    symbol.to_string_lossy().into_owned()
}

fn symbol_c_string(symbol: &str) -> Result<CString> {
    CString::new(symbol)
        .map_err(|_| LoomError::invalid(format!("native symbol name contains NUL: {symbol:?}")))
}
