FROM ros:noetic-ros-base

RUN apt-get update && apt-get install -y --no-install-recommends \
    ros-noetic-rplidar-ros \
    ros-noetic-octomap-server \
    ros-noetic-octomap-msgs \
    ros-noetic-laser-geometry \
    ros-noetic-tf2-ros \
    ros-noetic-topic-tools \
    ros-noetic-hector-slam \
    ros-noetic-pcl-ros \
    ros-noetic-pcl-conversions \
    ros-noetic-nodelet \
    python3-pip \
    python3-serial \
    && rm -rf /var/lib/apt/lists/*

RUN pip3 install paho-mqtt pyserial

WORKDIR /root/rover-ros
COPY scripts/ /root/rover-ros/scripts/
COPY run.sh /root/rover-ros/run.sh
RUN chmod +x /root/rover-ros/scripts/* /root/rover-ros/run.sh

ENTRYPOINT ["/bin/bash"]
