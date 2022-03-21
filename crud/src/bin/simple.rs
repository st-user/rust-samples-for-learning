use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use warp::{http::StatusCode, Filter, Rejection, Reply};

use crud::data::{Employee, EmployeeBody, ErrorMessage, Role};

struct IdGenerator {
    id: i64,
}

impl IdGenerator {
    fn gen(&mut self) -> i64 {
        self.id += 1;
        self.id
    }
}

type DB = Arc<Mutex<HashMap<i64, Employee>>>;
type IdCounter = Arc<Mutex<IdGenerator>>;

#[tokio::main]
async fn main() {
    let db: DB = Arc::new(Mutex::new(HashMap::new()));
    {
        let mut db = db.lock().unwrap();
        db.insert(
            0,
            Employee {
                id: 0,
                name: String::from("John"),
                role: Role::Admin,
            },
        );
        db.insert(
            1,
            Employee {
                id: 1,
                name: String::from("Bob"),
                role: Role::Agent,
            },
        );
        db.insert(
            2,
            Employee {
                id: 2,
                name: String::from("Alice"),
                role: Role::SuperAdmin,
            },
        );
    }
    let id_counter = Arc::new(Mutex::new(IdGenerator { id: 2 }));

    let context = warp::path("employees");

    let get_route = context
        .and(warp::get())
        .and(with_db(db.clone()))
        .and(warp::path::param::<i64>())
        .and_then(get_employee);

    let get_all_route = context
        .and(warp::get())
        .and(with_db(db.clone()))
        .and_then(get_all_employees);

    let post_route = context
        .and(warp::post())
        .and(with_db(db.clone()))
        .and(with_id_counter(id_counter.clone()))
        .and(warp::body::json::<EmployeeBody>())
        .and_then(post_employee);

    let put_route = context
        .and(warp::put())
        .and(with_db(db.clone()))
        .and(warp::path::param::<i64>())
        .and(warp::body::json::<EmployeeBody>())
        .and_then(put_employee);

    let delete_route = context
        .and(warp::delete())
        .and(with_db(db.clone()))
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

fn with_db(db: DB) -> impl Filter<Extract = (DB,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || db.clone())
}

fn with_id_counter(
    id_counter: IdCounter,
) -> impl Filter<Extract = (IdCounter,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || id_counter.clone())
}

async fn get_employee(db: DB, id: i64) -> Result<impl Reply, Rejection> {
    let data = db.lock().unwrap();
    match data.get(&id) {
        Some(employee) => Ok(ok_with_json(&employee)),
        None => Ok(not_found_json()),
    }
}

async fn get_all_employees(db: DB) -> Result<impl Reply, Rejection> {
    let data = db.lock().unwrap();
    let mut employees = Vec::new();
    for (_, employee) in data.iter() {
        employees.push(Employee::from(employee));
    }
    Ok(ok_with_json(&employees))
}

async fn post_employee(
    db: DB,
    id_counter: IdCounter,
    body: EmployeeBody,
) -> Result<impl Reply, Rejection> {
    let mut db = db.lock().unwrap();
    let mut id_counter = id_counter.lock().unwrap();
    let new_id = id_counter.gen();
    let employee = Employee {
        id: new_id,
        name: body.name,
        role: body.role,
    };
    let ret = Employee::from(&employee);
    db.insert(new_id, employee);

    Ok(ok_with_json(&ret))
}

async fn put_employee(db: DB, id: i64, body: EmployeeBody) -> Result<impl Reply, Rejection> {
    let mut db = db.lock().unwrap();

    if let None = db.get(&id) {
        return Ok(not_found_json());
    }

    let employee = Employee {
        id: id,
        name: body.name,
        role: body.role,
    };
    let ret = Employee::from(&employee);
    db.insert(id, employee);

    Ok(ok_with_json(&ret))
}

async fn delete_employee(db: DB, id: i64) -> Result<impl Reply, Rejection> {
    let mut db = db.lock().unwrap();

    if let Some(employee) = db.remove(&id) {
        Ok(ok_with_json(&employee))
    } else {
        Ok(not_found_json())
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
