import sys, time, tempfile
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python"))
import darudb
from darudb import field
from common import person

@darudb.collection("people")
class Person:
    id: int | None = None
    name: str
    email: str = field(unique=True)
    age: int = field(index=True)
    city: str
    score: float

SCHEMA = darudb.Schema(1, [Person])
d = Path(tempfile.mkdtemp())
db = darudb.Database.open(d / "c.darudb", schema=SCHEMA)
n = int(sys.argv[1])
objs = [Person(**person(i)) for i in range(n)]
t = time.perf_counter_ns()
for i in range(n):
    with db.write(durability="deferred") as txn:
        txn.collection(Person).insert(objs[i])
print("deferred insert", (time.perf_counter_ns() - t) / n / 1000, "us")
db.close()
