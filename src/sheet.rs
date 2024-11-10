use rsheet_lib::{
    cell_expr::{self, CellArgument},
    cell_value::{self, CellValue},
    cells,
    command::CellIdentifier,
    replies::Reply,
};
use std::collections::HashMap;

pub struct Sheet {
    cells: HashMap<u32, HashMap<u32, CellValue>>,
}

const LIST_MATRIX_SEPARATOR: char = '_';

impl Sheet {
    pub fn new() -> Sheet {
        Sheet {
            cells: HashMap::new(),
        }
    }

    pub fn set(&mut self, ident: CellIdentifier, expr: String) -> Reply {
        let expr = cell_expr::CellExpr::new(&expr);
        let mut vars = HashMap::new();
        expr.find_variable_names().iter().map(|name: &String| {});
        todo!()
    }

    fn name_to_cell_values(&self, name: &str) -> Result<CellArgument, String> {
        match name.split_once(LIST_MATRIX_SEPARATOR) {
            Some((start_name, end_name)) => {
                // TODO: handle matrix
                todo!()
            }
            None => {
                // TODO: handle single cell
                let ident = name.parse::<CellIdentifier>()?;
                return Ok(CellArgument::Value(self.get_cell_value(ident)));
            }
        }
    }

    pub fn get(&self, ident: CellIdentifier) -> Reply {
        let cell_name = format!("{}{}", cells::column_number_to_name(ident.col), ident.row);
        return Reply::Value(cell_name, self.get_cell_value(ident));
    }

    pub fn get_cell_value(&self, ident: CellIdentifier) -> CellValue {
        match self.cells.get(&ident.col) {
            Some(col) => match col.get(&ident.row) {
                Some(cell) => cell.clone(),
                None => CellValue::None,
            },
            _ => CellValue::None,
        }
    }
}
