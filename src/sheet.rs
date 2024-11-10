use rsheet_lib::{
    cell_expr::{self, CellArgument},
    cell_value::CellValue,
    cells,
    command::CellIdentifier,
    replies::Reply,
};
use std::collections::{HashMap, HashSet};

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

    pub fn set(&mut self, ident: &CellIdentifier, expr: String) -> Reply {
        let expr = cell_expr::CellExpr::new(&expr);
        let mut vars = HashMap::new();
        let mut dep_cells = HashSet::new();
        for name in expr.find_variable_names() {
            match self.name_to_cell_values(&name) {
                Ok((idents, argument)) => {
                    vars.insert(name, argument);
                    dep_cells.extend(idents);
                }
                Err(err) => {
                    return Reply::Error(err);
                }
            }
        }
        match expr.evaluate(&vars) {
            Ok(value) => {
                self.set_cell_value(ident, value.clone());
                Reply::Value(Self::ident_to_name(ident), value)
            }
            Err(_) => Reply::Error("Value dependent error value".to_string()),
        }
    }

    fn ident_to_name(ident: &CellIdentifier) -> String {
        format!(
            "{}{}",
            cells::column_number_to_name(ident.col),
            ident.row + 1
        )
    }

    fn name_to_cell_values(
        &self,
        name: &str,
    ) -> Result<(Vec<CellIdentifier>, CellArgument), String> {
        match name.split_once(LIST_MATRIX_SEPARATOR) {
            Some((start_name, end_name)) => {
                let start_ident = start_name.parse::<CellIdentifier>()?;
                let end_ident = end_name.parse::<CellIdentifier>()?;
                let mut matrix = Vec::new();
                let mut idents = Vec::new();
                for col in start_ident.col..=end_ident.col {
                    let mut col_vector = Vec::new();
                    for row in start_ident.row..=end_ident.row {
                        let ident = CellIdentifier { col, row };
                        col_vector.push(self.get_cell_value(&ident));
                        idents.push(ident);
                    }
                    matrix.push(col_vector);
                }
                if matrix.len() == 0 {
                    return Err("Empty matrix".to_string());
                }
                if matrix.len() == 1 {
                    // only one col
                    return Ok((idents, CellArgument::Vector(matrix[0].to_owned())));
                }
                if matrix[0].len() == 1 {
                    // test one row
                    let mut row_vector = Vec::new();
                    for ele in matrix {
                        if ele.len() != 1 {
                            // not a list
                            break;
                        }
                        row_vector.push(ele[0].to_owned());
                    }
                    return Ok((idents, CellArgument::Vector(row_vector)));
                }
                return Ok((idents, CellArgument::Matrix(matrix)));
            }
            None => {
                // TODO: handle single cell
                let ident = name.parse::<CellIdentifier>()?;
                return Ok((
                    vec![ident],
                    CellArgument::Value(self.get_cell_value(&ident)),
                ));
            }
        }
    }

    pub fn get(&self, ident: &CellIdentifier) -> Reply {
        return Reply::Value(Self::ident_to_name(ident), self.get_cell_value(ident));
    }

    fn get_cell_value(&self, ident: &CellIdentifier) -> CellValue {
        match self.cells.get(&ident.col) {
            Some(col) => match col.get(&ident.row) {
                Some(cell) => cell.clone(),
                None => CellValue::None,
            },
            _ => CellValue::None,
        }
    }

    fn set_cell_value(&mut self, ident: &CellIdentifier, value: CellValue) {
        self.cells
            .entry(ident.col)
            .or_insert_with(HashMap::new)
            .insert(ident.row, value);
    }
}
