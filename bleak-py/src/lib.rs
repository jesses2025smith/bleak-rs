mod bleak;

// use log::LevelFilter;
use pyo3::{
    pymodule,
    types::{PyModule, PyModuleMethods},
    wrap_pyfunction, Bound, PyResult, Python,
};
// use pyo3::exceptions::PyEnvironmentError;

#[pymodule]
fn bleak_py(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // rsutil::log::Log4rsConfig::default()
    //     .set_pattern("[{l}]: {m}{n}")
    //     .set_console_level(LevelFilter::Info)
    //     .initialize()
    //     .map_err(|e| PyEnvironmentError::new_err(e.to_string()))?;
    m.add_class::<bleak::BLEDevice>()?;
    m.add_class::<bleak::DeviceDiscover>()?;
    m.add_function(wrap_pyfunction!(bleak::discover, m)?)?;
    m.add_function(wrap_pyfunction!(bleak::find_device_by_address, m)?)?;
    // m.add_function(wrap_pyfunction!(bleak::find_device_by_filters, m)?)?;
    m.add_function(wrap_pyfunction!(bleak::find_device_by_name, m)?)?;

    Ok(())
}
