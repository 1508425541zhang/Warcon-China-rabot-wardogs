FROM python:3.12-slim
WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir numpy==1.26.4 && pip install --no-cache-dir torch==2.8.0 --index-url https://download.pytorch.org/whl/cpu
COPY . .
RUN useradd --uid 10001 --create-home model
USER model
ENV MODEL_HOST=0.0.0.0 MODEL_PORT=8091
EXPOSE 8091
CMD ["python", "server.py"]
