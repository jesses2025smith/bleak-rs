mod bleak;

use pyo3::{
    pymodule,
    types::{PyModule, PyModuleMethods},
    wrap_pyfunction, Bound, PyResult, Python,
};

#[pymodule]
fn bleak_py(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<bleak::BLEDevice>()?;
    m.add_function(wrap_pyfunction!(bleak::discover, m)?)?;
    m.add_function(wrap_pyfunction!(bleak::find_device_by_address, m)?)?;
    // m.add_function(wrap_pyfunction!(bleak::find_device_by_filters, m)?)?;
    m.add_function(wrap_pyfunction!(bleak::find_device_by_name, m)?)?;

    Ok(())
}
