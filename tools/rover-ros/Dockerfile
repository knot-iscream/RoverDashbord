FROM ros:noetic-ros-base

RUN apt-get update && apt-get install -y --no-install-recommends \
    ros-noetic-rplidar-ros \
    ros-noetic-octomap-server \
    ros-noetic-octomap-msgs \
    ros-noetic-laser-geometry \
    ros-noetic-tf2-ros \
    ros-noetic-topic-tools \
    python3-pip \
    && rm -rf /var/lib/apt/lists/*

RUN pip3 install paho-mqtt

WORKDIR /root/rover-ros
COPY scripts/ /root/rover-ros/scripts/
COPY run.sh /root/rover-ros/run.sh
RUN chmod +x /root/rover-ros/scripts/* /root/rover-ros/run.sh

ENTRYPOINT ["/bin/bash"]
