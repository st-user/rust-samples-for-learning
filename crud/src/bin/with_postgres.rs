use mobc::Pool;
use mobc_postgres::PgConnectionManager;
use postgres_types::{FromSql, ToSql};
use serde::Serialize;
use std::str::FromStr;
use tokio_postgres::{Config, NoTls, Row};

use warp::{http::StatusCode, Filter, Rejection, Reply};

use crud::data::{Employee, EmployeeBody, ErrorMessage, Role};

#[derive(Debug, ToSql, FromSql)]
#[postgres(name = "employee_role")]
enum EmployeeRole {
    SuperAdmin,
    Admin,
    Agent,
}
impl EmployeeRole {
    fn from(role: &Role) -> EmployeeRole {
        match role {
            Role::SuperAdmin => EmployeeRole::SuperAdmin,
            Role::Admin => EmployeeRole::Admin,
            Role::Agent => EmployeeRole::Agent,
        }
    }
    
    fn to_role(&self) -> Role {
        match self {
            EmployeeRole::SuperAdmin => Role::SuperAdmin,
            EmployeeRole::Admin => Role::Admin,
            EmployeeRole::Agent => Role::Agent,
        }
    }
}

type DBPool = mobc::Pool<mobc_postgres::PgConnectionManager<tokio_postgres::NoTls>>;

/// The Data Access Object(DAO) for Employee.
/// 
struct EmployeeDAO {
    db_pool: DBPool,
}

impl EmployeeDAO {

    /// Get all Employees.
    /// 
    /// This function is intended to be used for the `GET /employees` endpoint.
    /// 
    async fn find_all(&self) -> Result<Vec<Employee>, String> {
        let client = self.db_pool.get().await.map_err(|e| format!("{:?}", e))?;
        let sql = "
            SELECT
                    *
                FROM
                    employee
        ";

        let result = client
            .query(sql, &[])
            .await
            .map_err(|e| format!("{:?}", e))?;

        Ok(result.iter().map(row_to_employee).collect())
    }

    /// Get an Employee mapped to the given id.
    /// 
    /// This function is intended to be used for the `GET /employees/${id}` endpoint.
    /// 
    async fn find_by_id(&self, id: &i64) -> Result<Option<Employee>, String> {
        let client = self.db_pool.get().await.map_err(|e| format!("{:?}", e))?;
        let sql = "
            SELECT
                    *
                FROM
                    employee
                WHERE
                    id = $1
        ";

        let result = client
            .query(sql, &[id])
            .await
            .map_err(|e| format!("{:?}", e))?;

        Ok(result.get(0).map(row_to_employee))
    }

    /// Create an Employee with the given name and role.
    /// 
    /// The id field is generated automatically.
    /// This function is intended to be used for the `POST /employees` endpoint.
    /// 
    async fn insert(&self, name: &String, role: &EmployeeRole) -> Result<Employee, String> {
        let client = self.db_pool.get().await.map_err(|e| format!("{:?}", e))?;

        let sql = "
            INSERT
                INTO 
                    employee (
                        name, role
                    )
                VALUES
                    (
                        $1, $2
                    )
                RETURNING *
        ";

        let result = client
            .query(sql, &[name, role])
            .await
            .map_err(|e| format!("{:?}", e))?;

        if let Some(row) = result.get(0) {
            Ok(row_to_employee(row))
        } else {
            Result::Err(String::from(
                "Unexpectedly 'RETURNING *' didn't return a row.",
            ))
        }
    }

    /// Update an Employee mapped to the given id with the given name and role.
    /// 
    /// This function is intended to be used for the `PUT /employees/${id}` endpoint.
    /// 
    async fn update(
        &self,
        id: &i64,
        name: &String,
        role: &EmployeeRole,
    ) -> Result<Employee, (StatusCode, String)> {
        let mut client = self
            .db_pool
            .get()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{:?}", e)))?;

        let tran = client
            .transaction()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{:?}", e)))?;

        let select_sql = "
            SELECT
                    *
                FROM
                    employee
                WHERE
                    id = $1
                FOR UPDATE
        ";

        let select_result = tran
            .query(select_sql, &[id])
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{:?}", e)))?;

        if select_result.len() == 0 {
            let not_found = (
                StatusCode::NOT_FOUND,
                format!("The row corresponding to id:{} doesn't exist", id),
            );
            return Result::Err(not_found);
        }

        let update_sql = "
            UPDATE 
                    employee
                SET
                    name = $1,
                    role = $2
                WHERE
                    id = $3
                RETURNING *
        ";
        let result = tran
            .query(update_sql, &[name, role, id])
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{:?}", e)))?;

        if let Some(row) = result.get(0) {
            tran.commit()
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{:?}", e)))?;

            Ok(row_to_employee(row))
        } else {
            Result::Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                String::from("Unexpectedly 'RETURNING *' didn't return a row."),
            ))
        }
    }

    /// Delete an Employee mapped to the given id.
    /// 
    /// This function is intended to be used for the `DELETE /employees/${id}` endpoint.
    /// 
    async fn delete_by_id(&self, id: &i64) -> Result<Option<Employee>, String> {
        let client = self.db_pool.get().await.map_err(|e| format!("{:?}", e))?;

        let sql = "
            DELETE
                FROM
                    employee
                WHERE
                    id = $1
                RETURNING *
        ";

        let result = client
            .query(sql, &[id])
            .await
            .map_err(|e| format!("{:?}", e))?;

        Ok(result.get(0).map(row_to_employee))
    }
}

fn row_to_employee(row: &Row) -> Employee {
    let employee_role: EmployeeRole = row.get(2);
    Employee {
        id: row.get(0),
        name: row.get(1),
        role: employee_role.to_role(),
    }
}

fn init_db_pool() -> DBPool {
    let config = Config::from_str("postgresql://postgres:postgres@localhost:5555").unwrap();
    let connection_manager = PgConnectionManager::new(config, NoTls);

    Pool::builder().max_open(20).build(connection_manager)
}

async fn init_db(pool: &DBPool) {
    let client = pool.get().await.unwrap();

    let create_type_mod = "
        CREATE TYPE employee_role AS ENUM ('SuperAdmin', 'Admin', 'Agent')
    ";
    if let Err(e) = client.query(create_type_mod, &[]).await {
        println!("Type 'employee_role' may exist {:?}", e);
    }

    let create_table = "
        CREATE TABLE IF NOT EXISTS employee (
            id bigserial PRIMARY KEY,
            name varchar(100) NOT NULL,
            role employee_role
        )
    ";

    client.query(create_table, &[]).await.unwrap();
}

#[tokio::main]
async fn main() {
    let db_pool = init_db_pool();
    init_db(&db_pool).await;

    let context = warp::path("employees");

    let get_route = context
        .and(warp::get())
        .and(with_db(db_pool.clone()))
        .and(warp::path::param::<i64>())
        .and_then(get_employee);

    let get_all_route = context
        .and(warp::get())
        .and(with_db(db_pool.clone()))
        .and_then(get_all_employees);

    let post_route = context
        .and(warp::post())
        .and(with_db(db_pool.clone()))
        .and(warp::body::json::<EmployeeBody>())
        .and_then(post_employee);

    let put_route = context
        .and(warp::put())
        .and(with_db(db_pool.clone()))
        .and(warp::path::param::<i64>())
        .and(warp::body::json::<EmployeeBody>())
        .and_then(put_employee);

    let delete_route = context
        .and(warp::delete())
        .and(with_db(db_pool.clone()))
        .and(warp::path::param::<i64>())
        .and_then(delete_employee);

    let route = get_route
        .or(get_all_route)
        .or(post_route)
        .or(put_route)
        .or(delete_route)
        .recover(handle_rejection);

    warp::serve(route).run(([127, 0, 0, 1], 8080)).await;
}

fn with_db(
    db_pool: DBPool,
) -> impl Filter<Extract = (EmployeeDAO,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || EmployeeDAO {
        db_pool: db_pool.clone(),
    })
}

async fn get_employee(dao: EmployeeDAO, id: i64) -> Result<impl Reply, Rejection> {
    match dao.find_by_id(&id).await {
        Ok(employee) => match employee {
            Some(employee) => Ok(ok_with_json(&employee)),
            None => Ok(not_found_json()),
        },
        Err(e) => {
            println!("{:?}", e);
            Ok(internal_server_error_json())
        }
    }
}

async fn get_all_employees(dao: EmployeeDAO) -> Result<impl Reply, Rejection> {
    match dao.find_all().await {
        Ok(employees) => Ok(ok_with_json(&employees)),
        Err(e) => {
            println!("{:?}", e);
            Ok(internal_server_error_json())
        }
    }
}

async fn post_employee(dao: EmployeeDAO, body: EmployeeBody) -> Result<impl Reply, Rejection> {
    match dao
        .insert(&body.name, &EmployeeRole::from(&body.role))
        .await
    {
        Ok(employee) => Ok(ok_with_json(&employee)),
        Err(e) => {
            println!("{:?}", e);
            Ok(internal_server_error_json())
        }
    }
}

async fn put_employee(
    dao: EmployeeDAO,
    id: i64,
    body: EmployeeBody,
) -> Result<impl Reply, Rejection> {
    match dao
        .update(&id, &body.name, &EmployeeRole::from(&body.role))
        .await
    {
        Ok(employee) => Ok(ok_with_json(&employee)),
        Err(e) => {
            if e.0 == StatusCode::NOT_FOUND {
                println!("{:?}", e.1);
                Ok(not_found_json())
            } else {
                println!("{:?}", e.1);
                Ok(internal_server_error_json())
            }
        }
    }
}

async fn delete_employee(dao: EmployeeDAO, id: i64) -> Result<impl Reply, Rejection> {
    match dao.delete_by_id(&id).await {
        Ok(employee) => match employee {
            Some(employee) => Ok(ok_with_json(&employee)),
            None => Ok(not_found_json()),
        },
        Err(e) => {
            println!("{:?}", e);
            Ok(internal_server_error_json())
        }
    }
}

async fn handle_rejection(err: Rejection) -> Result<impl Reply, std::convert::Infallible> {
    println!("handle_rejection {:?}", err);
    Ok(warp::reply::with_status(
        "Internal Server Error",
        StatusCode::INTERNAL_SERVER_ERROR,
    ))
}

fn ok_with_json<T>(employee: &T) -> warp::reply::WithStatus<warp::reply::Json>
where
    T: Serialize,
{
    warp::reply::with_status(warp::reply::json(employee), StatusCode::OK)
}

fn not_found_json() -> warp::reply::WithStatus<warp::reply::Json> {
    warp::reply::with_status(
        warp::reply::json(&ErrorMessage {
            code: 404,
            message: String::from("Not Found"),
        }),
        StatusCode::NOT_FOUND,
    )
}

fn internal_server_error_json() -> warp::reply::WithStatus<warp::reply::Json> {
    warp::reply::with_status(
        warp::reply::json(&ErrorMessage {
            code: 500,
            message: String::from("Internal Server Error"),
        }),
        StatusCode::INTERNAL_SERVER_ERROR,
    )
}
