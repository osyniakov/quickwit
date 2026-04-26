FROM python:3.12-alpine
COPY es_compat_proxy.py /app/es_compat_proxy.py
WORKDIR /app
EXPOSE 9200
CMD ["python", "es_compat_proxy.py"]
