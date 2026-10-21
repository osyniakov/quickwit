"""Sends OpenLineage events describing a small ETL pipeline to Quickwit.

The script emits two kinds of lineage:
- configuration (static lineage): one `JobEvent` per job, describing its type,
  SQL query, source code location, owners, and its input and output datasets
  with their schemas.
- execution (runtime lineage): for each simulated daily run of the pipeline,
  `START` and `COMPLETE` (or `FAIL`) `RunEvent`s for the pipeline and each of
  its tasks, with the execution parameters, the parent run, the processing
  engine, the output statistics, and the error message of failed runs.

The OpenLineage client is configured with `openlineage.yml` (next to this
script) unless `OPENLINEAGE_CONFIG` or `OPENLINEAGE_URL` is set.
"""

import argparse
import os
import random
from datetime import datetime, timedelta, timezone
from pathlib import Path

from openlineage.client import OpenLineageClient
from openlineage.client.event_v2 import (
    InputDataset,
    Job,
    JobEvent,
    OutputDataset,
    Run,
    RunEvent,
    RunState,
)
from openlineage.client.facet_v2 import (
    documentation_job,
    error_message_run,
    execution_parameters_run,
    job_type_job,
    nominal_time_run,
    output_statistics_output_dataset,
    ownership_job,
    parent_run,
    processing_engine_run,
    schema_dataset,
    source_code_location_job,
    sql_job,
)
from openlineage.client.uuid import generate_new_uuid

PRODUCER = "https://github.com/quickwit-oss/quickwit/tree/main/config/tutorials/openlineage"
JOB_NAMESPACE = "quickwit-tutorial"
DATASET_NAMESPACE = "postgres://warehouse:5432"
PIPELINE_NAME = "daily_orders"

# Datasets and their schemas, as (column name, column type) pairs.
DATASETS = {
    "public.orders": [("order_id", "BIGINT"), ("customer_id", "BIGINT"), ("amount", "DECIMAL"), ("created_at", "TIMESTAMP")],
    "public.customers": [("customer_id", "BIGINT"), ("country", "VARCHAR")],
    "analytics.daily_revenue": [("day", "DATE"), ("country", "VARCHAR"), ("revenue", "DECIMAL")],
    "analytics.revenue_report": [("day", "DATE"), ("total_revenue", "DECIMAL")],
}

# Tasks of the pipeline, executed in order.
TASKS = [
    {
        "name": f"{PIPELINE_NAME}.aggregate_revenue",
        "description": "Aggregates the revenue of the day per country.",
        "inputs": ["public.orders", "public.customers"],
        "outputs": ["analytics.daily_revenue"],
        "sql": (
            "INSERT INTO analytics.daily_revenue "
            "SELECT DATE(o.created_at), c.country, SUM(o.amount) "
            "FROM public.orders o JOIN public.customers c USING (customer_id) "
            "WHERE DATE(o.created_at) = :day GROUP BY 1, 2"
        ),
        "error": "psycopg2.errors.DeadlockDetected: deadlock detected on relation public.orders",
    },
    {
        "name": f"{PIPELINE_NAME}.build_report",
        "description": "Builds the daily revenue report.",
        "inputs": ["analytics.daily_revenue"],
        "outputs": ["analytics.revenue_report"],
        "sql": (
            "INSERT INTO analytics.revenue_report "
            "SELECT day, SUM(revenue) FROM analytics.daily_revenue WHERE day = :day GROUP BY 1"
        ),
        "error": "psycopg2.errors.DivisionByZero: division by zero",
    },
]


def to_iso(timestamp: datetime) -> str:
    return timestamp.isoformat(timespec="milliseconds")


def schema_facet(dataset_name: str) -> schema_dataset.SchemaDatasetFacet:
    fields = [
        schema_dataset.SchemaDatasetFacetFields(name=name, type=column_type)
        for name, column_type in DATASETS[dataset_name]
    ]
    return schema_dataset.SchemaDatasetFacet(fields=fields)


def input_datasets(dataset_names: list[str]) -> list[InputDataset]:
    return [
        InputDataset(namespace=DATASET_NAMESPACE, name=name, facets={"schema": schema_facet(name)})
        for name in dataset_names
    ]


def output_datasets(dataset_names: list[str], row_count: int | None = None) -> list[OutputDataset]:
    outputs = []
    for name in dataset_names:
        output_facets = {}
        if row_count is not None:
            output_facets["outputStatistics"] = output_statistics_output_dataset.OutputStatisticsOutputDatasetFacet(
                rowCount=row_count
            )
        outputs.append(
            OutputDataset(
                namespace=DATASET_NAMESPACE,
                name=name,
                facets={"schema": schema_facet(name)},
                outputFacets=output_facets,
            )
        )
    return outputs


def job_type_facet(job_type: str) -> job_type_job.JobTypeJobFacet:
    return job_type_job.JobTypeJobFacet(processingType="BATCH", integration="PYTHON", jobType=job_type)


def configuration_events(event_time: datetime) -> list[JobEvent]:
    """Returns the static lineage of the pipeline and of its tasks."""
    owners = ownership_job.OwnershipJobFacet(owners=[ownership_job.Owner(name="team:data-platform", type="MAINTAINER")])
    source_code_location = source_code_location_job.SourceCodeLocationJobFacet(
        type="git",
        url=PRODUCER,
        repoUrl="https://github.com/quickwit-oss/quickwit.git",
        path="config/tutorials/openlineage/send_openlineage_events.py",
        branch="main",
    )
    pipeline_event = JobEvent(
        eventTime=to_iso(event_time),
        producer=PRODUCER,
        job=Job(
            namespace=JOB_NAMESPACE,
            name=PIPELINE_NAME,
            facets={
                "jobType": job_type_facet("DAG"),
                "documentation": documentation_job.DocumentationJobFacet(description="Computes the daily revenue report."),
                "ownership": owners,
                "sourceCodeLocation": source_code_location,
            },
        ),
    )
    events = [pipeline_event]
    for task in TASKS:
        task_event = JobEvent(
            eventTime=to_iso(event_time),
            producer=PRODUCER,
            job=Job(
                namespace=JOB_NAMESPACE,
                name=task["name"],
                facets={
                    "jobType": job_type_facet("TASK"),
                    "documentation": documentation_job.DocumentationJobFacet(description=task["description"]),
                    "ownership": owners,
                    "sourceCodeLocation": source_code_location,
                    "sql": sql_job.SQLJobFacet(query=task["sql"], dialect="postgres"),
                },
            ),
            inputs=input_datasets(task["inputs"]),
            outputs=output_datasets(task["outputs"]),
        )
        events.append(task_event)
    return events


def run_event(
    event_type: RunState,
    event_time: datetime,
    job_name: str,
    run_id: str,
    run_facets: dict,
    inputs: list[InputDataset] | None = None,
    outputs: list[OutputDataset] | None = None,
) -> RunEvent:
    return RunEvent(
        eventType=event_type,
        eventTime=to_iso(event_time),
        producer=PRODUCER,
        run=Run(runId=run_id, facets=run_facets),
        job=Job(namespace=JOB_NAMESPACE, name=job_name),
        inputs=inputs or [],
        outputs=outputs or [],
    )


def execution_events(day: datetime, failure_rate: float, rng: random.Random) -> list[RunEvent]:
    """Returns the runtime lineage of one run of the pipeline, scheduled on `day`."""
    nominal_time = nominal_time_run.NominalTimeRunFacet(
        nominalStartTime=to_iso(day), nominalEndTime=to_iso(day + timedelta(days=1))
    )
    processing_engine = processing_engine_run.ProcessingEngineRunFacet(version="16.4", name="postgres")
    pipeline_run_id = str(generate_new_uuid())
    pipeline_facets = {
        "nominalTime": nominal_time,
        "processingEngine": processing_engine,
        "executionParameters": execution_parameters_run.ExecutionParametersRunFacet(
            parameters=[
                execution_parameters_run.ExecutionParameter(key="day", value=day.date().isoformat()),
                execution_parameters_run.ExecutionParameter(key="retries", value="0"),
            ]
        ),
    }
    parent = parent_run.ParentRunFacet(
        run=parent_run.Run(runId=pipeline_run_id),
        job=parent_run.Job(namespace=JOB_NAMESPACE, name=PIPELINE_NAME),
    )
    # The pipeline is triggered one hour after the end of the day it processes.
    current_time = day + timedelta(days=1, hours=1, seconds=rng.randint(0, 59))
    events = [run_event(RunState.START, current_time, PIPELINE_NAME, pipeline_run_id, pipeline_facets)]
    pipeline_state = RunState.COMPLETE
    for task in TASKS:
        task_run_id = str(generate_new_uuid())
        task_facets = {
            "parent": parent,
            "nominalTime": nominal_time,
            "processingEngine": processing_engine,
            "executionParameters": execution_parameters_run.ExecutionParametersRunFacet(
                parameters=[
                    execution_parameters_run.ExecutionParameter(key="day", value=day.date().isoformat()),
                    execution_parameters_run.ExecutionParameter(
                        key="statement_timeout", value="600", description="Statement timeout in seconds."
                    ),
                ]
            ),
        }
        inputs = input_datasets(task["inputs"])
        events.append(run_event(RunState.START, current_time, task["name"], task_run_id, task_facets, inputs=inputs))
        current_time += timedelta(seconds=rng.randint(5, 300))
        if rng.random() < failure_rate:
            failure_facets = {
                "parent": parent,
                "errorMessage": error_message_run.ErrorMessageRunFacet(
                    message=task["error"], programmingLanguage="PYTHON"
                ),
            }
            events.append(run_event(RunState.FAIL, current_time, task["name"], task_run_id, failure_facets))
            pipeline_state = RunState.FAIL
            # The next tasks of the pipeline are not executed.
            break
        row_count = rng.randint(10, 10_000)
        outputs = output_datasets(task["outputs"], row_count=row_count)
        events.append(
            run_event(RunState.COMPLETE, current_time, task["name"], task_run_id, {"parent": parent}, inputs, outputs)
        )
    events.append(run_event(pipeline_state, current_time, PIPELINE_NAME, pipeline_run_id, {}))
    return events


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--days", type=int, default=30, help="Number of daily pipeline runs to simulate.")
    parser.add_argument("--failure-rate", type=float, default=0.1, help="Probability for a task run to fail.")
    parser.add_argument("--seed", type=int, default=None, help="Seed of the random generator.")
    args = parser.parse_args()

    if "OPENLINEAGE_CONFIG" not in os.environ and "OPENLINEAGE_URL" not in os.environ:
        os.environ["OPENLINEAGE_CONFIG"] = str(Path(__file__).resolve().parent / "openlineage.yml")
    client = OpenLineageClient()
    rng = random.Random(args.seed)
    today = datetime.now(timezone.utc).replace(hour=0, minute=0, second=0, microsecond=0)
    first_day = today - timedelta(days=args.days)

    num_events = 0
    for event in configuration_events(first_day):
        client.emit(event)
        num_events += 1
    print(f"Sent {num_events} configuration events.")

    for day_offset in range(args.days):
        day = first_day + timedelta(days=day_offset)
        for event in execution_events(day, args.failure_rate, rng):
            client.emit(event)
            num_events += 1
    print(f"Sent {num_events} events in total ({args.days} pipeline runs).")
    client.close()


if __name__ == "__main__":
    main()
