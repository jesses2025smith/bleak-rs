mod bleak;

use pyo3::{
    Bound, PyResult, Python, pymodule,
    types::{PyModule, PyModuleMethods},
    wrap_pyfunction,
};

#[pymodule]
fn bleak_py(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<bleak::BLEDevice>()?;
    m.add_class::<bleak::BLEClient>()?;
    m.add_function(wrap_pyfunction!(bleak::find_device_by_address, m)?)?;
    // m.add_function(wrap_pyfunction!(bleak::find_device_by_filters, m)?)?;
    m.add_function(wrap_pyfunction!(bleak::find_device_by_name, m)?)?;

    Ok(())
}
