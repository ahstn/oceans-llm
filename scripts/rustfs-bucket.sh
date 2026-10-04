#!/usr/bin/env bash
set -euo pipefail

: "${RUSTFS_ACCESS_KEY:?Run mise run rustfs:setup first}"
: "${RUSTFS_SECRET_KEY:?Run mise run rustfs:setup first}"
: "${OCEANS_SKILLS_S3_ENDPOINT:?Select the rustfs mise environment}"
: "${OCEANS_RUSTFS_BUCKET:?Select the rustfs mise environment}"
: "${OCEANS_RUSTFS_REGION:?Select the rustfs mise environment}"

# Keep these credentials scoped to this process, separate from AWS provider keys.
export AWS_ACCESS_KEY_ID="$RUSTFS_ACCESS_KEY"
export AWS_SECRET_ACCESS_KEY="$RUSTFS_SECRET_KEY"
export AWS_DEFAULT_REGION="$OCEANS_RUSTFS_REGION"
export AWS_EC2_METADATA_DISABLED=true
export AWS_PAGER=""
unset AWS_SESSION_TOKEN AWS_SECURITY_TOKEN AWS_PROFILE

aws_args=(--endpoint-url "$OCEANS_SKILLS_S3_ENDPOINT" --region "$OCEANS_RUSTFS_REGION" --cli-connect-timeout 5 --cli-read-timeout 10)
error_file="$(mktemp "${TMPDIR:-/tmp}/oceans-rustfs-bucket.XXXXXX")"
trap 'rm -f "$error_file"' EXIT

if aws "${aws_args[@]}" s3api head-bucket --bucket "$OCEANS_RUSTFS_BUCKET" 2>"$error_file"; then
  echo "RustFS bucket $OCEANS_RUSTFS_BUCKET is ready."
  exit 0
fi

case "$(cat "$error_file")" in
  *"(404)"*|*"(NoSuchBucket)"*|*"(Not Found)"*) ;;
  *) cat "$error_file" >&2; exit 1 ;;
esac

create_args=(--bucket "$OCEANS_RUSTFS_BUCKET")
if [[ "$OCEANS_RUSTFS_REGION" != "us-east-1" ]]; then
  create_args+=(--create-bucket-configuration "LocationConstraint=$OCEANS_RUSTFS_REGION")
fi
if ! aws "${aws_args[@]}" s3api create-bucket "${create_args[@]}" > /dev/null 2>"$error_file"; then
  case "$(cat "$error_file")" in
    *"(BucketAlreadyOwnedByYou)"*) ;;
    *) cat "$error_file" >&2; exit 1 ;;
  esac
fi
aws "${aws_args[@]}" s3api head-bucket --bucket "$OCEANS_RUSTFS_BUCKET"
echo "RustFS bucket $OCEANS_RUSTFS_BUCKET is ready."
