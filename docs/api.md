# The API

What the service answers over HTTP, and what it calls on other units. Every other unit reaches the
service through this API and nothing else.

## A refused request

- **The status is a plain HTTP status code:** 400 for a request that is wrong as asked, 403 for one
  the asker may not make, 404 for something that does not exist, 409 for a request that conflicts
  with the present state, and 500 for a failure on the service's side, such as its database.
- **The body is a JSON object of exactly two keys:**
  - `error`: one English sentence for a person, naming why;
  - `code`: a stable snake_case code for a machine. A caller decides on the code and never on the
    sentence.
- A database failure is refused with status 500 and code `database`.
